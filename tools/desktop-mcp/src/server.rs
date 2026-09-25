//! The MCP surface: one `#[tool]` per operation, each validating its
//! arguments and handing the work to the desktop thread.
//!
//! Every reply is a typed value with a published output schema, sent as
//! structured content and, for clients that show only text, as the same
//! JSON in a text block; `screenshot` alone sends an image and text and no
//! structured content, since clients that receive structured content drop
//! the rest. A failure is a tool error whose structured content carries
//! the code, retry policy and effect ([`ToolError::payload`]).
//!
//! Every tool takes the request's cancellation (rmcp hands it over as a
//! `CancellationToken`): a call cancelled while it waits for the desktop
//! thread or the process pool never runs, and a waiting loop stops.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, JsonObject, ServerCapabilities, ServerConfig,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::a11y::{self, Action, Node};
use crate::capture::Window;
use crate::desktop::Activated;
use crate::error::{Retry, ToolError, ToolResult};
use crate::params::{
    ActivateParams, Args, ButtonArg, ClickParams, DragParams, ElementParams, FindParams, Format,
    KeyParams, KillParams, LaunchParams, ListWindowsParams, MoveMouseParams, Scope,
    ScreenshotParams, ScrollParams, SetValueParams, TargetArg, TreeParams, TypeTextParams,
    WaitForParams, WaitForWindowParams,
};
use crate::process::{Children, LaunchSpec};
use crate::worker::Worker;

/// How often `wait_for` and `wait_for_window` re-check.
const POLL: Duration = Duration::from_millis(250);

/// The longest one tree read (`accessibility_tree`, `find`) runs before it
/// returns what it has, marked `truncated`: a slow or hung provider must not
/// hold the one desktop thread every tool shares. Counted from when the read
/// starts on that thread, not from when it was queued; a provider call
/// already running when it passes finishes first (up to 5 s more).
const READ_DEADLINE: Duration = Duration::from_secs(10);

/// How many times `launch` tries to bind a started pid while the desktop
/// queue is full (every [`POLL`]).
const BIND_ATTEMPTS: u32 = 40;

/// How long `launch` waits in all to bind the pid it started.
const BIND_WAIT: Duration = Duration::from_secs(10);

/// The least time one `wait_for` poll reads for.
const MIN_READ: Duration = Duration::from_secs(1);

/// How long `launch` waits for the OS to start a program.
const SPAWN_WAIT: Duration = Duration::from_secs(30);

const INSTRUCTIONS: &str = "\
Drives desktop applications like a person with a screen reader. One server per desktop: it \
shares the one pointer and keyboard with the person at it, and a call the client timed out on \
still runs, so read before re-sending an action.

Typical loop: list_windows (or launch, then wait_for_window) -> activate_window -> \
accessibility_tree / find / screenshot -> invoke / toggle / set_value / select / expand \
(preferred: no pointer, works when covered) or click / type_text / key -> wait_for to confirm \
the result (an element, its state, or that it is gone).

Handles are session-scoped: windows `w3` from list_windows or wait_for_window, elements `e12` \
from accessibility_tree, find or wait_for, screenshots `s2` from screenshot. The same window or \
element keeps its handle across reads; one whose window, element or process is gone answers \
`gone` and is never re-bound. A pid is bound to the process it named when first handed out.

Safety: click, drag, scroll, type_text and key require a target, `window` or `pid`. The server \
refuses the input unless that window is in front (and, for coordinates, holds the point \
uncovered; for keys, holds keyboard focus in its own process), so input never lands in another \
application, apart from the few milliseconds between the last check and the event. Popup menus \
and drop-downs are windows of their own: target them with pid. Shell hotkeys (the Windows key, \
alt+tab, ctrl+esc, the language switch) are refused.

Errors carry a `code` to branch on, `retry` (`never`, `soon`, `when_appears`) and, when part \
of an action already went out, an `effect` (`partial` with the count, `may_have_run`, `ran`, \
`incidental`): look before retrying any of those.

Coordinates are screen coordinates everywhere (physical pixels on Windows, points on macOS); \
pass a screenshot's id with image pixels instead and the server maps them. A read visits at \
most 5000 elements (500 reported by default; read a subtree with `root`) and stops after 10 s; \
strings longer than 4096 characters end in an ellipsis; `truncated: true` means the read did \
not see the whole tree, so an empty find then proves nothing. Accessibility tools use UI \
Automation and are Windows-only for now; window listing and screenshots work on Windows and \
macOS; on macOS targeted input is refused for now (the server cannot verify what covers a \
point or holds keyboard focus there); on Linux none of them is available yet.";

/// The MCP server.
#[derive(Debug, Clone)]
pub struct DesktopServer {
    worker: Worker,
    children: Arc<Children>,
    tool_router: ToolRouter<Self>,
}

/// A failed call: the same error envelope in text and structured content.
fn failure(e: &ToolError) -> CallToolResult {
    let payload = e.payload();
    let mut result = CallToolResult::error(vec![ContentBlock::text(payload.to_string())]);
    result.structured_content = Some(payload);
    result
}

/// A successful call: the value as structured content and as text.
fn reply<T: Serialize>(value: &T) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => {
            let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
            result.structured_content = Some(value);
            result
        }
        Err(e) => failure(&ToolError::platform("serializing the reply", e)),
    }
}

/// Wraps a result for the agent.
fn respond<T: Serialize>(result: ToolResult<T>) -> CallToolResult {
    match result {
        Ok(value) => reply(&value),
        Err(e) => failure(&e),
    }
}

/// The value of a validation, or the tool error it failed with.
macro_rules! valid {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(e) => return failure(&e),
        }
    };
}

/// An error as data inside a successful reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorInfo {
    /// The code, as in a failed call.
    pub code: &'static str,
    /// The readable message.
    pub message: String,
}

impl ErrorInfo {
    fn of(e: &ToolError) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

/// The output schema of a tool whose reply is `T`, widened so a failed call
/// (whose structured content is `{"error": ...}`) conforms too: a client
/// validating error results against the schema (the TypeScript SDK does)
/// must not reject them.
fn schema<T: JsonSchema>() -> Arc<JsonObject> {
    // For serialization: a field left out at its default
    // (`skip_serializing_if`) is optional there, and an `Option` that is
    // always sent is required and nullable.
    let mut root = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    let object = root
        .as_object_mut()
        .expect("BUG: a struct's schema is an object");
    let required = object.remove("required").unwrap_or_else(|| json!([]));
    object.insert("type".into(), json!("object"));
    let properties = object
        .entry("properties")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("BUG: properties is an object");
    properties.insert(
        "error".into(),
        json!({
            "type": "object",
            "description": "Present on a failed call (isError): the code to branch on, the message, the retry policy and, when part of an action went out, its effect.",
            "properties": {
                "code": { "type": "string" },
                "message": { "type": "string" },
                "retry": { "type": "string", "enum": ["never", "soon", "when_appears"] },
                "effect": { "type": "object" }
            },
            "required": ["code", "message", "retry"]
        }),
    );
    object.insert(
        "anyOf".into(),
        json!([{ "required": required }, { "required": ["error"] }]),
    );
    Arc::new(object.clone())
}

/// A screen point.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct Point {
    /// Screen x.
    pub x: i32,
    /// Screen y.
    pub y: i32,
}

impl From<(i32, i32)> for Point {
    fn from((x, y): (i32, i32)) -> Self {
        Self { x, y }
    }
}

/// `list_windows` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WindowsReply {
    /// Top-level windows, front to back.
    pub windows: Vec<Window>,
}

/// `launch` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LaunchReply {
    /// The new process; pass to wait_for_window, and as `pid` elsewhere.
    pub pid: u32,
    /// Whether the pid is bound as a target in this session. When not
    /// (`note` says why), target the process's windows by their handles
    /// from list_windows, or kill it.
    pub bound: bool,
    /// Why the pid is not bound, when it is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `wait_for_window` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WindowReply {
    /// The first window the process showed that matches.
    pub window: Window,
}

/// How a process ended.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct Exit {
    /// The exit code, when the OS reports one.
    pub code: Option<i32>,
}

/// `kill` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KillReply {
    /// The process.
    pub pid: u32,
    /// Whether it had already exited before the kill.
    pub already_exited: bool,
    /// How it ended.
    pub exited: Exit,
}

/// `accessibility_tree` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TreeReply {
    /// The trees as one line per element (`format: outline`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    /// The trees as nodes, one per window read (`format: json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<Node>>,
    /// Elements reported.
    pub count: usize,
    /// Whether the read left anything out (a budget, the depth, a provider
    /// failing partway): then read a subtree with `root`, or more nodes.
    pub truncated: bool,
}

/// `find` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FindReply {
    /// The matches, one line each (`format: outline`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    /// The matches as nodes without children (`format: json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches: Option<Vec<Node>>,
    /// Matches returned.
    pub count: usize,
    /// Matches found in all, when more than `limit`.
    pub total: usize,
    /// Whether the read did not see the whole tree: then no match is not
    /// proof of absence.
    pub truncated: bool,
}

/// `wait_for` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WaitReply {
    /// The first element matching the condition; absent for a `gone` wait.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<Node>,
    /// For a `gone` wait: nothing matches any more.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
}

/// The reply of an element action.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ActReply {
    /// The element afterwards (`gone: true` when the action removed it),
    /// or absent when its state could not be read back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<Node>,
    /// Why the element could not be read back: the action itself ran, so do
    /// not repeat it; read the tree instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readback_failed: Option<ErrorInfo>,
}

/// `click` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ClickReply {
    /// Where the click landed.
    pub at: Point,
    /// The button.
    pub button: ButtonArg,
    /// Whether it was a double-click.
    pub double: bool,
}

/// `move_mouse` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MoveReply {
    /// Where the pointer was asked to go.
    pub requested: Point,
    /// Where it is now, when the OS reports it.
    pub at: Option<Point>,
}

/// `drag` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DragReply {
    /// Where the button went down.
    pub from: Point,
    /// Where it was released.
    pub to: Point,
}

/// `scroll` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScrollReply {
    /// Where the wheel turned.
    pub at: Point,
    /// Horizontal notches sent.
    pub dx: i32,
    /// Vertical notches sent.
    pub dy: i32,
}

/// `type_text` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TypedReply {
    /// Characters typed.
    pub characters: usize,
}

/// `key` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KeyReply {
    /// The combo, as parsed.
    pub combo: String,
    /// How many times it was pressed.
    pub repeat: u32,
}

/// Runs blocking process work (spawning, waiting for an exit) off the async
/// runtime's thread, which also carries the stdio transport. Work whose
/// request was cancelled before it started does nothing, as on the desktop
/// thread.
///
/// On a plain thread, not the runtime's blocking pool: a spawn stuck on an
/// unreachable network executable cannot be aborted, and the runtime's
/// teardown would wait for a pool task, keeping the server alive after the
/// client left. A plain thread ends with the process.
///
/// With `admit`, the work takes a thread of that [`Pool`]; with
/// `give_up`, the caller stops waiting once that time passes or `ct` fires,
/// and the work (which sees the same `ct`, cancelled then) undoes itself
/// when it finishes. Exactly one side handles a result: one the caller
/// stopped waiting for is either picked up anyway (it arrived first) or
/// handed to `undo` on the work's thread.
async fn blocking<T: Send + 'static>(
    ct: &CancellationToken,
    admit: Option<&'static Pool>,
    give_up: Option<Duration>,
    work: impl FnOnce() -> ToolResult<T> + Send + 'static,
    undo: impl FnOnce(T) + Send + 'static,
) -> ToolResult<T> {
    if ct.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    // Admission like the desktop queue's: a spawn stuck on a network path
    // holds its thread, and unbounded threads would exhaust the process.
    let slot = admit.map(ProcessSlot::take).transpose()?;
    let (reply, mut result) = tokio::sync::oneshot::channel();
    let job_ct = ct.clone();
    std::thread::Builder::new()
        .name("desktop-process".into())
        .spawn(move || {
            let _slot = slot;
            // Cancelled before it started: say so, rather than drop the reply
            // and have it read as a panic.
            if job_ct.is_cancelled() {
                let _ = reply.send(Err(ToolError::Cancelled));
                return;
            }
            if reply.is_closed() {
                return;
            }
            // The caller stopped waiting before this arrived: undone here.
            if let Err(Ok(unsent)) = reply.send(work()) {
                undo(unsent);
            }
        })
        .map_err(|e| ToolError::platform("starting process work", e))?;
    let panicked = |_| {
        ToolError::platform("running process work", "it panicked").after(
            crate::error::Effect::MayHaveRun,
            "process work may have changed the process before panicking; inspect it before retrying",
        )
    };
    let Some(limit) = give_up else {
        return result.await.map_err(panicked)?;
    };
    let gave_up = tokio::select! {
        reply = &mut result => return reply.map_err(panicked)?,
        () = ct.cancelled() => Err(ToolError::Cancelled),
        () = tokio::time::sleep(limit) => {
            ct.cancel();
            // Not "retry shortly": the same path would hang again.
            Err(ToolError::platform(
                "starting the program",
                format!(
                    "it did not start within {} s (a slow or unreachable path?); it is ended if it does start",
                    limit.as_secs()
                ),
            ))
        }
    };
    // Closed first, then read: a result sent before the close is the
    // caller's to handle; one sent after it fails and goes to `undo`.
    result.close();
    match result.try_recv() {
        // The work saw the cancel this wait sent when it gave up.
        Ok(Err(ToolError::Cancelled)) | Err(_) => gave_up,
        Ok(arrived) => arrived,
    }
}

/// A bound on the threads one kind of process work may hold at once. Launch
/// and kill have their own, so launches stuck on unreachable paths never
/// keep kills from running.
struct Pool {
    running: std::sync::atomic::AtomicUsize,
    limit: usize,
    /// The error when full.
    full: fn(usize) -> ToolError,
}

static LAUNCHES: Pool = Pool {
    running: std::sync::atomic::AtomicUsize::new(0),
    limit: 8,
    // Passing, if slowly: a stuck spawn is given up on after `SPAWN_WAIT`.
    full: |n| {
        ToolError::Busy(format!(
            "{n} launches are still starting their programs (stuck on unreachable paths?); each is given up on after {} s",
            SPAWN_WAIT.as_secs()
        ))
    },
};

static KILLS: Pool = Pool {
    running: std::sync::atomic::AtomicUsize::new(0),
    limit: 16,
    full: |n| ToolError::Busy(format!("{n} kills are waiting for their processes to exit")),
};

/// One thread of a [`Pool`], given back when dropped.
struct ProcessSlot(&'static Pool);

impl ProcessSlot {
    fn take(pool: &'static Pool) -> ToolResult<Self> {
        use std::sync::atomic::Ordering;
        pool.running
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < pool.limit).then_some(n + 1)
            })
            .map(|_| Self(pool))
            .map_err(|_| (pool.full)(pool.limit))
    }
}

impl Drop for ProcessSlot {
    fn drop(&mut self) {
        self.0
            .running
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Sleeps `duration`, or less when `ct` fires; whether it fired.
async fn pause(ct: &CancellationToken, duration: Duration) -> bool {
    tokio::select! {
        () = tokio::time::sleep(duration) => false,
        () = ct.cancelled() => true,
    }
}

/// Withdraw queued reads at their deadline. The worker checks the deadline
/// too, because its thread may claim the job before the async timer is polled.
/// A read already running finishes and reports its actual outcome.
async fn before_deadline<T: Send + 'static>(
    worker: &Worker,
    ct: &CancellationToken,
    deadline: Instant,
    read: impl FnOnce(&mut crate::desktop::Desktop) -> ToolResult<T> + Send + 'static,
) -> ToolResult<T> {
    let lookup = ct.child_token();
    let run = worker.run(&lookup, move |desktop| {
        if Instant::now() >= deadline {
            return Err(ToolError::Cancelled);
        }
        read(desktop)
    });
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => result,
        () = tokio::time::sleep(deadline.saturating_duration_since(Instant::now())) => {
            lookup.cancel();
            run.await
        }
    }
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

    /// A launch whose request was cancelled: the client drops the reply of a
    /// cancelled request (rmcp does not send it), so it never learns the
    /// pid, and the process is ended rather than left running unnamed.
    async fn void_launch(&self, pid: u32, launch: u64) -> CallToolResult {
        let children = Arc::clone(&self.children);
        // No pool: it must run even when every kill thread is taken, and it
        // is bounded by the reap wait.
        let ended = blocking(
            &CancellationToken::new(),
            None,
            None,
            move || children.abandon(pid, launch),
            |_| {},
        )
        .await;
        match ended {
            Ok(_) => tracing::info!(pid, "ended the process of a cancelled launch"),
            Err(e) => tracing::warn!(pid, "could not end the process of a cancelled launch: {e}"),
        }
        failure(&ToolError::Cancelled)
    }

    async fn act(&self, ct: &CancellationToken, element: String, action: Action) -> CallToolResult {
        respond(
            self.worker
                .run(ct, move |d| d.act(&element, &action))
                .await
                .map(|outcome| ActReply {
                    element: outcome.element,
                    readback_failed: outcome.readback.as_ref().map(ErrorInfo::of),
                }),
        )
    }
}

#[tool_router]
impl DesktopServer {
    #[tool(
        title = "List windows",
        description = "List top-level windows, front to back: id (a session handle, pass as `window`), pid, app_name, title, rect, is_minimized, is_focused, and targetable with untargetable_reason when this session cannot target it. Handles are stable for the same window; a closed window's handle answers `gone`.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<WindowsReply>()
    )]
    async fn list_windows(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ListWindowsParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let windows = self
            .worker
            .run(&ct, move |d| {
                d.list_windows(p.title_contains.as_deref(), p.pid)
            })
            .await;
        respond(windows.map(|windows| WindowsReply { windows }))
    }

    #[tool(
        title = "Launch a program",
        description = "Start a program (stdio discarded) and return its pid, bound as a target in this session once the desktop thread is free (`bound`; wait_for_window binds it later when it could not be at once). Then wait_for_window to get its window. The server kills every launched process when it exits: on Windows also everything those start, even when the server itself is killed; elsewhere only the launched processes, on a clean exit. A launch the client cancels ends the process it started, since the pid is never delivered.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = schema::<LaunchReply>()
    )]
    async fn launch(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<LaunchParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        valid!(p.validate());
        let spec = LaunchSpec {
            program: p.program,
            args: p.args,
            cwd: p.cwd.map(Into::into),
            env: p.env.into_iter().collect(),
        };
        let children = Arc::clone(&self.children);
        let unsent = Arc::clone(&self.children);
        // Its own token: cancelled with the request, or when the wait for
        // the spawn gives up, and read by the spawn thread when it finishes.
        let spawn_ct = ct.child_token();
        let finished = spawn_ct.clone();
        let (pid, started, launch) = valid!(
            blocking(
                &spawn_ct,
                Some(&LAUNCHES),
                Some(SPAWN_WAIT),
                move || {
                    let (pid, started, launch) = children.launch(&spec)?;
                    // Nobody will be told this pid: end the process rather
                    // than leave one running that no one can name.
                    if finished.is_cancelled() {
                        let _ = children.abandon(pid, launch);
                        return Err(ToolError::Cancelled);
                    }
                    Ok((pid, started, launch))
                },
                // Started, but the wait was over before the pid got back.
                move |(pid, _, launch)| {
                    let _ = unsent.abandon(pid, launch);
                },
            )
            .await
        );
        if ct.is_cancelled() {
            return self.void_launch(pid, launch).await;
        }
        // Bind the pid to the identity read at the spawn. The process runs
        // now whatever happens to this request, so this step is not skipped;
        // a full queue is waited out rather than failing the binding.
        // Bounded as a whole: a desktop thread stuck in a platform call
        // would otherwise hold the pid back from the caller for good.
        let bind_until = Instant::now() + BIND_WAIT;
        let never = CancellationToken::new();
        let mut bound = Err(ToolError::ShuttingDown);
        for _ in 0..BIND_ATTEMPTS {
            let left = bind_until.saturating_duration_since(Instant::now());
            let attempt = self.worker.run(&never, move |d| {
                if d.bind_launched(pid, started) {
                    Ok(())
                } else {
                    Err(ToolError::NotSupported(
                        "this pid was handed out before for another process, or the OS reports no start time".into(),
                    ))
                }
            });
            bound = tokio::time::timeout(left, attempt)
                .await
                .unwrap_or_else(|_| {
                    Err(ToolError::Busy(
                        "the desktop thread is still busy with an earlier call".into(),
                    ))
                });
            if Instant::now() >= bind_until {
                break;
            }
            // Refused for good, or skipped because the server is stopping:
            // stop. Anything passing (a full queue) is retried.
            if !bound.as_ref().is_err_and(|e| e.retry() == Retry::Soon) {
                break;
            }
            if pause(&ct, POLL).await {
                return self.void_launch(pid, launch).await;
            }
        }
        if ct.is_cancelled() {
            return self.void_launch(pid, launch).await;
        }
        reply(&LaunchReply {
            pid,
            bound: bound.is_ok(),
            note: bound.err().map(|e| e.to_string()),
        })
    }

    #[tool(
        title = "Wait for a window",
        description = "Wait until a process this session launched or listed shows a top-level window (optionally one whose title contains a text), polling every 250 ms, and return it with its handle. Some apps (Windows 11 Notepad) hand off to another process: then no window ever appears under this pid, the process exits (`gone`), and list_windows finds the window. Windows only for now.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<WindowReply>()
    )]
    async fn wait_for_window(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<WaitForWindowParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (pid, needle, timeout) = valid!(p.validate());
        if !cfg!(target_os = "windows") {
            return failure(&ToolError::NotSupported(format!(
                "wait_for_window is not supported on {}: the OS reports no process start time, so a window under the pid cannot be told from a later process's; find it with list_windows",
                std::env::consts::OS
            )));
        }
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(exit) = self.children.exited(pid) {
                return failure(&ToolError::Gone {
                    handle: pid.to_string(),
                    kind: crate::error::HandleKind::Process,
                    why: format!(
                        "it exited{} before showing a window; if it handed off to another process, find that one with list_windows",
                        exit.code
                            .map_or(String::new(), |c| format!(" with code {c}"))
                    ),
                });
            }
            // Bounded by what is left of the wait: a lookup still queued
            // behind other desktop work when the time is up is withdrawn
            // (one already running finishes; it cannot be stopped halfway).
            // A pid this session launched but could not bind then (the
            // desktop thread was busy) is bound now, from the start time
            // read at the spawn.
            let started = self.children.launched_start(pid);
            let windows = before_deadline(&self.worker, &ct, deadline, move |d| {
                if started.is_some() {
                    d.bind_launched(pid, started);
                }
                d.windows_of(pid)
            })
            .await;
            // Withdrawn at the deadline, not by the client: the wait is over.
            if matches!(windows, Err(ToolError::Cancelled)) && !ct.is_cancelled() {
                return failure(&ToolError::Timeout {
                    timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    what: format!(
                        "a window of process {pid} (the desktop was busy with other calls)"
                    ),
                    summary: "(the process is running; list_windows shows what it has)".into(),
                });
            }
            match windows {
                Ok(None) => {
                    return failure(&ToolError::Gone {
                        handle: pid.to_string(),
                        kind: crate::error::HandleKind::Process,
                        why: "it exited before showing a window; if it handed off to another process, find that one with list_windows".into(),
                    });
                }
                Ok(Some(windows)) => {
                    let matching: Vec<Window> = windows
                        .into_iter()
                        .filter(|w| {
                            needle
                                .as_deref()
                                .is_none_or(|n| a11y::fold(&w.title).contains(n))
                        })
                        .collect();
                    if let Some(window) = matching
                        .iter()
                        .find(|w| w.targetable)
                        .or_else(|| matching.first())
                    {
                        return reply(&WindowReply {
                            window: window.clone(),
                        });
                    }
                }
                Err(e) if e.retry() == Retry::Soon => {}
                Err(e) => return failure(&e),
            }
            if Instant::now() >= deadline {
                return failure(&ToolError::Timeout {
                    timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    what: match &needle {
                        Some(n) => format!("a window of process {pid} whose title contains {n:?}"),
                        None => format!("a window of process {pid}"),
                    },
                    summary: "(the process is running; list_windows shows what it has)".into(),
                });
            }
            if pause(
                &ct,
                POLL.min(deadline.saturating_duration_since(Instant::now())),
            )
            .await
            {
                return failure(&ToolError::Cancelled);
            }
        }
    }

    #[tool(
        title = "Kill a launched process",
        description = "End a process started by launch in this session (other pids are refused). Ends that process only; what it started itself ends when the server exits (on Windows). Reports already_exited for one that ended on its own or was killed before.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<KillReply>()
    )]
    async fn kill(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<KillParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let children = Arc::clone(&self.children);
        respond(
            blocking(
                &ct,
                Some(&KILLS),
                None,
                move || children.kill(p.pid),
                |_| {},
            )
            .await
            .map(|killed| KillReply {
                pid: killed.pid,
                already_exited: killed.already_exited,
                exited: Exit {
                    code: killed.exit_code,
                },
            }),
        )
    }

    #[tool(
        title = "Take a screenshot",
        description = "Capture a window (`window` or `pid`; covered windows are captured where the OS allows), a monitor (0-based index), or the primary monitor (no target), downscaled to max_side (default 1920, at most 4096) on its longer side. Returns a PNG and, as text, JSON metadata: id (`s2`; pass it with image-pixel x/y to click, move_mouse, scroll or drag, and the server maps them), width, height, the captured screen rect (source) and scale_x/scale_y (image px per screen unit: screen x = source.x + image x / scale_x). Refused if the window moved during the capture or no longer belongs to the process it was listed for.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn screenshot(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ScreenshotParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let target = valid!(p.target());
        let max_side = p.max_side();
        let (shot, id) = valid!(
            self.worker
                .run(&ct, move |d| d.screenshot(target, Some(max_side)))
                .await
        );
        let meta = json!({
            "id": id,
            "width": shot.width,
            "height": shot.height,
            "source": shot.source,
            "scale_x": shot.scale_x,
            "scale_y": shot.scale_y,
        });
        // No structured content: clients that receive it drop the image.
        CallToolResult::success(vec![
            ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(&shot.png),
                "image/png",
            ),
            ContentBlock::text(meta.to_string()),
        ])
    }

    #[tool(
        title = "Read the accessibility tree",
        description = "Read the element tree of a window (`window`), of all a process's windows and popups (`pid`), or under one element (`root`), max_depth levels deep (default 30) and max_nodes elements (default 500, at most 5000). Default `format: outline`: one line per element, `- role \"name\" [ref=e12] [state...] [actions=...]`, with `[window=w3]` on each root; `format: json` gives nodes: id, role, native_role, name, value, automation_id, class_name, rect, disabled, focused, focusable, checked, expanded, selected, actions, window, children, omitted_children. truncated: true when the read left anything out.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<TreeReply>()
    )]
    async fn accessibility_tree(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<TreeParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (scope, depth, max_nodes, format) = valid!(p.validate());
        let read = self
            .worker
            .run(&ct, move |d| {
                d.tree(&scope, depth, max_nodes, Instant::now() + READ_DEADLINE)
            })
            .await;
        respond(read.map(|r| {
            let count = a11y::count(&r.roots);
            match format {
                Format::Outline => TreeReply {
                    outline: Some(a11y::outline(&r.roots)),
                    roots: None,
                    count,
                    truncated: r.truncated,
                },
                Format::Json => TreeReply {
                    outline: None,
                    roots: Some(r.roots),
                    count,
                    truncated: r.truncated,
                },
            }
        }))
    }

    #[tool(
        title = "Find elements",
        description = "Find elements in a window, a process or under an element (`root`) by name (exact), name_contains (case-insensitive), role (`button`, `text_input`, ... or the OS's own name) and/or automation_id; all given criteria must match. Returns at most `limit` matches (default 50) as an outline or as nodes without children, and truncated: true when the read did not see the whole tree (then no match is not proof of absence).",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<FindReply>()
    )]
    async fn find(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<FindParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (scope, query, limit, format) = valid!(p.validate());
        let found = self
            .worker
            .run(&ct, move |d| {
                d.find(&scope, &query, Instant::now() + READ_DEADLINE)
            })
            .await;
        respond(found.map(|(matches, read)| {
            let total = matches.len();
            let matches: Vec<Node> = matches.into_iter().take(limit).collect();
            let count = matches.len();
            match format {
                Format::Outline => FindReply {
                    outline: Some(a11y::outline(&matches)),
                    matches: None,
                    count,
                    total,
                    truncated: read.truncated,
                },
                Format::Json => FindReply {
                    outline: None,
                    matches: Some(matches),
                    count,
                    total,
                    truncated: read.truncated,
                },
            }
        }))
    }

    #[tool(
        title = "Wait for an element or a state",
        description = "Wait, polling every 250 ms, until an element matching the criteria (as for find, or one `element` by id) exists and is in `state` (checked, expanded, selected, focused, disabled, value, value_contains), or with `gone: true` until no element matches (a dialog closed, an item deleted). Returns the element, or a timeout error with a summary of the last tree seen. With pid, windows that appear or close meanwhile are followed; with window, a window that closes ends the wait as `gone`. The reply can come later than timeout_ms by one read, and by however long other calls hold the desktop thread.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<WaitReply>()
    )]
    async fn wait_for(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<WaitForParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let wait = valid!(p.validate());
        let deadline = Instant::now() + wait.timeout;
        let mut summary = String::from("(no tree read: the target had no window)");
        let follows = matches!(wait.scope, Scope::Target(TargetArg::Pid(_)));
        loop {
            let (scope, query) = (wait.scope.clone(), wait.query.clone());
            let read = self
                .worker
                .run(&ct, move |d| {
                    // Counted from when the read starts, not when it queued,
                    // and never so short that a poll the queue delayed past
                    // the wait's end reads nothing at all.
                    let now = Instant::now();
                    let read_deadline = deadline.max(now + MIN_READ).min(now + READ_DEADLINE);
                    d.find(&scope, &query, read_deadline)
                })
                .await;
            match read {
                Ok((matches, read)) => {
                    if wait.gone {
                        // Nothing matched in a whole read: gone. A cut read
                        // may have missed it, so it keeps waiting.
                        if matches.is_empty() && !read.truncated {
                            return reply(&WaitReply {
                                element: None,
                                gone: true,
                            });
                        }
                    } else if let Some(first) = matches
                        .into_iter()
                        .find(|n| wait.state.as_ref().is_none_or(|s| s.holds(n)))
                    {
                        return reply(&WaitReply {
                            element: Some(first),
                            gone: false,
                        });
                    }
                    summary = a11y::summarize(&read.roots, 60);
                    if read.truncated {
                        summary
                            .push_str("\n(the read was truncated: it did not see the whole tree)");
                    }
                }
                // What was waited on to be gone is gone with its window or
                // its root element.
                Err(e) if wait.gone && (e.code() == "gone" || e.code() == "not_found") => {
                    return reply(&WaitReply {
                        element: None,
                        gone: true,
                    });
                }
                // A pid with no window yet (a splash screen closed) is worth
                // waiting for; a window target that closed is not.
                Err(e) if e.retry() == Retry::WhenAppears && follows => {}
                // Passing: a full queue, a window that changed hands mid-read.
                Err(e) if e.retry() == Retry::Soon => {}
                Err(e) => return failure(&e),
            }
            if Instant::now() >= deadline {
                return failure(&ToolError::Timeout {
                    timeout_ms: u64::try_from(wait.timeout.as_millis()).unwrap_or(u64::MAX),
                    what: wait.describe(),
                    summary,
                });
            }
            if pause(
                &ct,
                POLL.min(deadline.saturating_duration_since(Instant::now())),
            )
            .await
            {
                return failure(&ToolError::Cancelled);
            }
        }
    }

    #[tool(
        title = "Invoke an element",
        description = "Invoke (press) an element through its accessibility action. No pointer involved; works even when the window is covered. Returns the element afterwards, or readback_failed when the action ran but its state could not be read back (then do not repeat it). An error with effect `may_have_run` means: read the tree before retrying.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn invoke(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::Invoke).await
    }

    #[tool(
        title = "Toggle an element",
        description = "Flip a checkbox or switch through its accessibility action (not idempotent: two calls flip it back). Returns the element with its new `checked`.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn toggle(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::Toggle).await
    }

    #[tool(
        title = "Set an element's value",
        description = "Replace an element's value (at most 100000 characters) through its accessibility action: text fields take the text, sliders a number. Returns the element afterwards.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn set_value(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<SetValueParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        valid!(p.validate());
        self.act(&ct, p.element, Action::SetValue(p.value)).await
    }

    #[tool(
        title = "Focus an element",
        description = "Give an element keyboard focus through the accessibility API. Succeeds once keyboard focus is on the element or inside it; an element that accepts the request but keeps no focus is an error.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn focus(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::Focus).await
    }

    #[tool(
        title = "Select an element",
        description = "Select an item (list item, tab, radio button) through its accessibility action.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn select(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::Select).await
    }

    #[tool(
        title = "Expand an element",
        description = "Open a collapsible element (a combo box, a tree item, a menu) through its accessibility action; then read the tree or find to see what it exposed.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn expand(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::Expand).await
    }

    #[tool(
        title = "Collapse an element",
        description = "Close an expanded element through its accessibility action.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn collapse(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::Collapse).await
    }

    #[tool(
        title = "Scroll an element into view",
        description = "Scroll an element's container until the element is visible, through its accessibility action (no pointer); returns the element with its new rect.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<ActReply>()
    )]
    async fn scroll_into_view(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ElementParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        self.act(&ct, p.element, Action::ScrollIntoView).await
    }

    #[tool(
        title = "Click",
        description = "Real mouse click at an element's clickable point (`element`), a screen point (`x`, `y`) or a screenshot pixel (`screenshot`, `x`, `y`). Requires `window` or `pid`: refused unless that window is in front and holds the point. button: left|right|middle (logical: swapped buttons are honoured); double for a double-click.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<ClickReply>()
    )]
    async fn click(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ClickParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (at, target) = valid!(p.validate());
        let (button, double) = (p.button, p.double);
        respond(
            self.worker
                .run(&ct, move |d| d.click(&at, button.into(), double, target))
                .await
                .map(|at| ClickReply {
                    at: at.into(),
                    button,
                    double,
                }),
        )
    }

    #[tool(
        title = "Move the pointer",
        description = "Move the pointer to an element, a screen point or a screenshot pixel; returns where it ended up. Sends no press, so it takes no safety target; refused while a button is still held from a failed release.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<MoveReply>()
    )]
    async fn move_mouse(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<MoveMouseParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let at = valid!(p.validate());
        respond(
            self.worker
                .run(&ct, move |d| d.move_mouse(&at))
                .await
                .map(|(requested, at)| MoveReply {
                    requested: requested.into(),
                    at: at.map(Into::into),
                }),
        )
    }

    #[tool(
        title = "Drag",
        description = "Left-button drag from one location to another (each an element, a screen point or a screenshot pixel) over duration_ms. Requires `window` or `pid`: refused unless that window is in front and holds every point of the drag, checked before the press and again before each step. If that stops holding partway, the button is released at the last point verified inside the target; otherwise the release, the one event that must go out, happens where the pointer is, and the error says so.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<DragReply>()
    )]
    async fn drag(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<DragParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (from, to, target, duration) = valid!(p.validate());
        respond(
            self.worker
                .run(&ct, move |d| d.drag(&from, &to, duration, target))
                .await
                .map(|(from, to)| DragReply {
                    from: from.into(),
                    to: to.into(),
                }),
        )
    }

    #[tool(
        title = "Scroll",
        description = "Turn the wheel over an element, a screen point or a screenshot pixel: dy notches (positive = down), dx notches (positive = right), at most 100 each. Requires `window` or `pid`: refused unless that window is in front and holds the point.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<ScrollReply>()
    )]
    async fn scroll(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ScrollParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (at, target) = valid!(p.validate());
        let (dx, dy) = (p.dx, p.dy);
        respond(
            self.worker
                .run(&ct, move |d| d.scroll(&at, dx, dy, target))
                .await
                .map(|at| ScrollReply {
                    at: at.into(),
                    dx,
                    dy,
                }),
        )
    }

    #[tool(
        title = "Type text",
        description = "Type text (at most 10000 characters) into the focused control as real keystrokes, layout-independent; a newline is Enter, a tab is Tab. Requires `window` or `pid`: refused unless that window is in front and holds keyboard focus in its own process. If it stops partway, the error's effect says how many characters went out.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<TypedReply>()
    )]
    async fn type_text(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<TypeTextParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let target = valid!(p.validate());
        respond(
            self.worker
                .run(&ct, move |d| d.type_text(&p.text, target))
                .await
                .map(|characters| TypedReply { characters }),
        )
    }

    #[tool(
        title = "Press a key",
        description = "Press a key or combo as real keystrokes: enter, tab, esc, f5, ctrl+shift+s, alt+f4, cmd+q, ctrl+plus (modifiers: ctrl shift alt meta/win/cmd; on Windows a character that needs Shift or AltGr on the layout gets it). repeat presses it N times. Requires `window` or `pid`: refused unless that window is in front and holds keyboard focus in its own process; shell hotkeys are refused.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = schema::<KeyReply>()
    )]
    async fn key(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<KeyParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (combo, repeat, target) = valid!(p.validate());
        let shown = combo.to_string();
        respond(
            self.worker
                .run(&ct, move |d| d.key(&combo, repeat, target))
                .await
                .map(|()| KeyReply {
                    combo: shown,
                    repeat,
                }),
        )
    }

    #[tool(
        title = "Activate a window",
        description = "Bring a window (`window`) or a process's window (`pid`) to the foreground, restoring it if minimized. Reports became_foreground: Windows can refuse focus changes, so check it before sending input. Windows only for now.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = schema::<Activated>()
    )]
    async fn activate_window(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ActivateParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let target = valid!(p.validate());
        respond(self.worker.run(&ct, move |d| d.activate(target)).await)
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

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn expired_window_lookup_never_starts() {
        use std::future::Future;
        use std::sync::mpsc;
        use std::task::{Context, Waker};

        let worker = Worker::spawn().expect("BUG: worker starts");
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&ran);
        let ct = CancellationToken::new();
        let (release, hold) = mpsc::channel();
        let mut blocker = std::pin::pin!(worker.run(&ct, move |_| {
            let _ = hold.recv();
            Ok(())
        }));
        let mut context = Context::from_waker(Waker::noop());
        assert!(blocker.as_mut().poll(&mut context).is_pending());

        let deadline = Instant::now() + Duration::from_secs(1);
        let mut call = std::pin::pin!(before_deadline(&worker, &ct, deadline, move |_| {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }));
        assert!(call.as_mut().poll(&mut context).is_pending());
        let (done, processed) = mpsc::channel();
        let mut barrier = std::pin::pin!(worker.run(&ct, move |_| {
            let _ = done.send(());
            Ok(())
        }));
        assert!(barrier.as_mut().poll(&mut context).is_pending());
        // Keep the runtime from polling the timer until the worker has
        // passed the expired job: its own deadline check must reject it.
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        release.send(()).expect("BUG: blocker waits");
        processed
            .recv_timeout(Duration::from_secs(5))
            .expect("BUG: barrier runs");
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert!(matches!(call.await, Err(ToolError::Cancelled)));
        blocker.await.expect("BUG: blocker completes");
        barrier.await.expect("BUG: barrier completes");
        assert!(!ct.is_cancelled(), "the deadline belongs to the lookup");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn panicking_process_work_reports_uncertain_effects() {
        let result = blocking::<()>(
            &CancellationToken::new(),
            None,
            None,
            || panic!("injected failure after process work started"),
            |()| {},
        )
        .await;
        let error = result.expect_err("BUG: injected panic must fail");
        assert_eq!(error.payload()["error"]["effect"]["kind"], "may_have_run");
    }

    #[test]
    fn process_threads_are_admitted_up_to_the_bound() {
        let held: Vec<ProcessSlot> = (0..LAUNCHES.limit)
            .map(|_| ProcessSlot::take(&LAUNCHES).expect("BUG: below the bound"))
            .collect();
        assert!(matches!(
            ProcessSlot::take(&LAUNCHES),
            Err(ToolError::Busy(_))
        ));
        // Kills have threads of their own while launches are stuck.
        assert!(ProcessSlot::take(&KILLS).is_ok());
        drop(held);
        assert!(ProcessSlot::take(&LAUNCHES).is_ok());
    }

    /// An output schema admits the reply and a failure alike, and never
    /// requires the success fields of a failed call.
    #[test]
    fn output_schemas_admit_success_and_failure() {
        let schema = schema::<KillReply>();
        let value = Value::Object((*schema).clone());
        assert_eq!(value["type"], "object");
        assert!(value["properties"]["pid"].is_object(), "{value}");
        assert!(value["properties"]["error"].is_object(), "{value}");
        assert!(value.get("required").is_none(), "{value}");
        let branches = value["anyOf"].as_array().expect("BUG: anyOf");
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[1]["required"], json!(["error"]));
        assert!(
            branches[0]["required"]
                .as_array()
                .is_some_and(|r| r.iter().any(|f| f == "pid")),
            "{value}"
        );
    }

    /// A field left out at its default is not required by the published
    /// schema, so a reply that leaves it out still conforms.
    #[test]
    fn skipped_fields_are_not_required() {
        let wait = Value::Object((*schema::<WaitReply>()).clone());
        let required = &wait["anyOf"][0]["required"];
        assert!(
            !required
                .as_array()
                .is_some_and(|r| r.iter().any(|f| f == "gone")),
            "{wait}"
        );
        let node = &wait["$defs"]["Node"];
        let required: Vec<&str> = node["required"]
            .as_array()
            .expect("BUG: Node has required fields")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(required, ["id", "role", "native_role"], "{node}");
    }

    /// A failure's structured content is the error envelope; a reply's is
    /// the value, with the same JSON as text.
    #[test]
    fn replies_carry_structured_content_and_text() {
        let ok = reply(&TypedReply { characters: 3 });
        assert_eq!(ok.structured_content, Some(json!({ "characters": 3 })));
        let text = match &ok.content[0] {
            ContentBlock::Text(t) => t.text.clone(),
            other => panic!("not text: {other:?}"),
        };
        assert_eq!(text, r#"{"characters":3}"#);
        let bad = failure(&ToolError::Busy("queue".into()));
        assert_eq!(bad.is_error, Some(true));
        assert_eq!(
            bad.structured_content
                .as_ref()
                .map(|v| v["error"]["code"].clone()),
            Some(json!("busy"))
        );
    }

    #[test]
    fn text_only_clients_receive_error_codes_and_partial_effects() {
        let error = ToolError::Busy("input interrupted".into()).after(
            crate::error::Effect::Partial {
                sent: 1,
                total: 2,
                unit: "clicks",
            },
            "one click completed; inspect before retrying",
        );
        let result = failure(&error);
        let ContentBlock::Text(text) = &result.content[0] else {
            panic!("BUG: failure includes a text envelope");
        };
        let payload: Value = serde_json::from_str(&text.text).expect("BUG: error text is JSON");
        assert_eq!(Some(&payload), result.structured_content.as_ref());
        assert_eq!(payload["error"]["code"], "busy");
        assert_eq!(payload["error"]["retry"], "never");
        assert_eq!(payload["error"]["effect"]["kind"], "partial");
        assert_eq!(payload["error"]["effect"]["sent"], 1);
        assert_eq!(payload["error"]["effect"]["total"], 2);
    }
}
