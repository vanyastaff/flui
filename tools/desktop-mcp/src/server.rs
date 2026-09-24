//! The MCP surface: one `#[tool]` per operation, each validating its
//! arguments and handing the work to the desktop thread.
//!
//! Every tool takes the request's cancellation (rmcp hands it over as a
//! `CancellationToken`): a call cancelled while it waits for the desktop
//! thread or the process pool never runs, and a waiting loop stops.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::a11y::{self, Action};
use crate::error::{ToolError, ToolResult};
use crate::params::{
    ActivateParams, Args, ClickParams, DragParams, ElementParams, FindParams, KeyParams,
    KillParams, LaunchParams, ListWindowsParams, MoveMouseParams, ScreenshotParams, ScrollParams,
    SetValueParams, Target, TreeParams, TypeTextParams, WaitForParams, required_target,
};
use crate::process::{Children, LaunchSpec};
use crate::worker::Worker;

/// How often `wait_for` and `launch` re-check.
const POLL: Duration = Duration::from_millis(250);

/// The longest one tree read (`accessibility_tree`, `find`) runs before it
/// returns what it has, marked `truncated`: a slow or hung provider must not
/// hold the one desktop thread every tool shares. Counted from when the read
/// starts on that thread, not from when it was queued.
const READ_DEADLINE: Duration = Duration::from_secs(10);

/// How many times `launch` tries to bind a started pid while the desktop
/// queue is full (every [`POLL`]).
const BIND_ATTEMPTS: u32 = 40;

/// How long `launch` waits in all to bind the pid it started.
const BIND_WAIT: Duration = Duration::from_secs(10);

/// The least time one `wait_for` poll reads for.
const MIN_READ: Duration = Duration::from_secs(1);

const INSTRUCTIONS: &str = "\
Drives desktop applications like a person with a screen reader. Coordinates are screen \
coordinates everywhere (window rects, element rects, input): physical pixels on Windows, \
points on macOS. A screenshot reports scale_x/scale_y to map its pixels back to them.

Typical loop: list_windows (or launch) -> activate_window -> accessibility_tree / find / \
screenshot -> invoke / toggle / set_value (preferred: no pointer, works when covered) or \
click / type_text / key -> read the tree again to confirm.

Safety: always pass window_id or pid to click, drag, scroll, type_text and key. The server \
then refuses the input unless that window is in front (and, for coordinates, holds the \
point uncovered; for keys, holds keyboard focus in its own process), so input never lands in \
another application. Window ids and pids are bound to the process they named when this \
session handed them out, and refused once that process is gone. Element clicks always \
require the element's own window to be under the point and its process in front. Popup \
menus and drop-downs are windows of their own: target them with pid, not window_id. Shell \
hotkeys (the Windows key, alt+tab, ctrl+esc, the language switch) are refused with a safety \
target. An error that says part of an action already went out, or that it may have run, \
means: look before retrying.

Element ids (e12) are session handles from accessibility_tree, find and wait_for; the same \
element keeps its id across reads (an id whose element or process is gone answers stale, \
even if UI Automation reuses its identity). A read visits at most 5000 elements and stops \
after 10 s; strings longer than 4096 characters end in an ellipsis; a reply with truncated: \
true did not see the whole tree, so an empty find result then does not mean the element is \
absent. Accessibility tools use UI Automation and are Windows-only for now. Window listing, \
screenshots and input work on Windows and macOS; on macOS targeted input is refused for now \
(the server cannot verify what covers a point or holds keyboard focus there). On Linux none \
of them is available yet.";

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

/// The value of a validation, or the tool error it failed with.
macro_rules! valid {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(e) => return respond::<Value>(Err(e)),
        }
    };
}

fn element_reply(result: ToolResult<a11y::Node>) -> CallToolResult {
    respond(result.map(|node| json!({ "element": node })))
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
    let panicked = |_| ToolError::platform("running process work", "it panicked");
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
        Ok(arrived) => arrived,
        Err(_) => gave_up,
    }
}

/// How long `launch` waits for the OS to start a program.
const SPAWN_WAIT: Duration = Duration::from_secs(30);

/// A bound on the threads one kind of process work may hold at once. Launch
/// and kill have their own, so launches stuck on unreachable paths never
/// keep kills from running.
struct Pool {
    running: std::sync::atomic::AtomicUsize,
    limit: usize,
    /// The error when full: a launch stays stuck as long as its path does
    /// (not worth retrying); a kill is over within its reap bound (it is).
    full: fn(usize) -> ToolError,
}

static LAUNCHES: Pool = Pool {
    running: std::sync::atomic::AtomicUsize::new(0),
    limit: 8,
    full: |n| {
        ToolError::NotSupported(format!(
            "{n} launches are still starting their programs (stuck on unreachable paths?); launch is unavailable until one finishes"
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
        respond::<Value>(Err(ToolError::Cancelled))
    }

    async fn act(&self, ct: &CancellationToken, element: String, action: Action) -> CallToolResult {
        element_reply(self.worker.run(ct, move |d| d.act(&element, &action)).await)
    }
}

#[tool_router]
impl DesktopServer {
    #[tool(
        description = "List top-level windows: id (pass as window_id), pid, app_name, title, rect, is_minimized, is_focused. Front to back.",
        annotations(read_only_hint = true)
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
        respond(windows.map(|w| json!({ "windows": w })))
    }

    #[tool(
        description = "Start a program (stdio discarded). Returns its pid; with wait_for_window_ms (at most 120000), also its first window, or its exit code if it exits first. The server kills every launched process when it exits: on Windows also everything those start, even when the server itself is killed; elsewhere only the launched processes, on a clean exit. A launch the client cancels ends the process it started, since the pid is never delivered."
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
        let mut bound = Err(ToolError::Cancelled);
        for _ in 0..BIND_ATTEMPTS {
            let left = bind_until.saturating_duration_since(Instant::now());
            let attempt = self
                .worker
                .run(&never, move |d| {
                    if d.bind_launched(pid, started) {
                        Ok(())
                    } else {
                        Err(ToolError::InvalidArgument(
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
            // stop. Anything else (a full queue) is retried.
            if matches!(
                bound,
                Err(ToolError::InvalidArgument(_) | ToolError::Cancelled)
            ) {
                break;
            }
            if bound.is_ok() {
                break;
            }
            tokio::time::sleep(POLL).await;
        }
        if ct.is_cancelled() {
            return self.void_launch(pid, launch).await;
        }
        if let Err(e) = bound {
            // Skipped because the server is stopping, not cancelled by the
            // client: the process is ended with the rest at shutdown.
            let why = if matches!(e, ToolError::Cancelled) {
                "the server is shutting down".to_owned()
            } else {
                e.to_string()
            };
            return respond(Ok(json!({
                "pid": pid,
                "window": null,
                "note": format!("started, but its pid was not bound as a target ({why}); target its windows by window_id from list_windows, or kill it"),
            })));
        }
        let Some(ms) = p.wait_for_window_ms else {
            return respond(Ok(json!({ "pid": pid })));
        };
        if started.is_none() {
            return respond(Ok(json!({
                "pid": pid,
                "window": null,
                "note": "this OS reports no process start time, so a window under this pid cannot be told from a later process's; find it with list_windows",
            })));
        }
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            if let Some(exit) = self.children.exited(pid) {
                return respond(Ok(json!({
                    "pid": pid,
                    "window": null,
                    "exit_code": exit.code,
                    "note": "the process exited before it showed a window; if it handed off to another process, find that one with list_windows",
                })));
            }
            // Bounded by what is left of the wait: a lookup still queued
            // behind other desktop work when the time is up is withdrawn
            // (one already running finishes; it cannot be stopped halfway).
            let lookup = ct.child_token();
            let run = self
                .worker
                .run(&lookup, move |d| d.launched_windows(pid, started));
            tokio::pin!(run);
            let windows = tokio::select! {
                windows = &mut run => windows,
                () = tokio::time::sleep(deadline.saturating_duration_since(Instant::now())) => {
                    lookup.cancel();
                    run.await
                }
            };
            // Cancelled while the lookup ran: it finished and answered, but
            // the reply will not reach the client, so neither does the pid.
            if ct.is_cancelled() {
                return self.void_launch(pid, launch).await;
            }
            // Withdrawn at the deadline, not by the client or shutdown.
            if matches!(windows, Err(ToolError::Cancelled))
                && lookup.is_cancelled()
                && !crate::desktop::stopping()
            {
                return respond(Ok(json!({
                    "pid": pid,
                    "window": null,
                    "note": "no window for this pid within the wait (the desktop was busy with other calls); see list_windows",
                })));
            }
            match windows {
                // Gone, and the pid may already be another process's: its
                // windows are not this launch's.
                Ok(None) => {
                    return respond(Ok(json!({
                        "pid": pid,
                        "window": null,
                        "exit_code": self.children.exited(pid).and_then(|exit| exit.code),
                        "note": "the process exited before it showed a window; if it handed off to another process, find that one with list_windows",
                    })));
                }
                // Its first window still open (one that closed while listed,
                // a splash screen, was dropped and the wait goes on); one
                // whose own id cannot be targeted says so, and the pid
                // reaches it.
                Ok(Some(w)) if !w.is_empty() => {
                    let first = w.into_iter().min_by_key(|w| w.not_targetable.is_some());
                    return respond(Ok(json!({ "pid": pid, "window": first })));
                }
                Err(ToolError::Cancelled) if ct.is_cancelled() => {
                    return self.void_launch(pid, launch).await;
                }
                // Skipped because the server is stopping: the pid still goes
                // back, and the process is ended with the rest.
                Err(ToolError::Cancelled) => {
                    return respond(Ok(json!({
                        "pid": pid,
                        "window": null,
                        "note": "the server is shutting down; the process is ended with it",
                    })));
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
                Ok(Some(_)) if Instant::now() >= deadline => {
                    return respond(Ok(json!({
                        "pid": pid,
                        "window": null,
                        "note": "no window for this pid yet; it may hand off to another process, see list_windows",
                    })));
                }
                Ok(Some(_)) => {
                    if pause(
                        &ct,
                        POLL.min(deadline.saturating_duration_since(Instant::now())),
                    )
                    .await
                    {
                        return self.void_launch(pid, launch).await;
                    }
                    if Instant::now() >= deadline {
                        return respond(Ok(json!({
                            "pid": pid,
                            "window": null,
                            "note": "no window for this pid yet; it may hand off to another process, see list_windows",
                        })));
                    }
                }
            }
        }
    }

    #[tool(
        description = "End a process started by launch in this session (other pids are refused). Ends that process only; what it started itself ends when the server exits (on Windows). Reports already_exited for one that ended on its own or was killed before."
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
            .await,
        )
    }

    #[tool(
        description = "Capture a window (window_id or pid; covered windows are captured where the OS allows), a monitor (0-based index), or the primary monitor (no target). The image is downscaled to max_side (default 1920, at most 4096) on its longer side. Returns a PNG plus, as structured content, its size, the captured screen rect (source) and scale_x/scale_y (image px per screen unit, measured from the pixels: 2 on a Retina display, below 1 when downscaled): screen x = source.x + image x / scale_x. Refused if the window moved during the capture or no longer belongs to the process it was listed for.",
        annotations(read_only_hint = true)
    )]
    async fn screenshot(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ScreenshotParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let target = valid!(p.target());
        let max_side = p.max_side();
        let shot = valid!(
            self.worker
                .run(&ct, move |d| d.screenshot(target, Some(max_side)))
                .await
        );
        let meta = json!({
            "width": shot.width,
            "height": shot.height,
            "source": shot.source,
            "scale_x": shot.scale_x,
            "scale_y": shot.scale_y,
        });
        let mut result = CallToolResult::success(vec![
            ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(&shot.png),
                "image/png",
            ),
            ContentBlock::text(meta.to_string()),
        ]);
        result.structured_content = Some(meta);
        result
    }

    #[tool(
        description = "Read the accessibility tree of a window (window_id) or of all a process's windows and popups (pid), max_depth levels deep (default 30, at most 200). Nodes: id (e12, for later calls), role, name, value, automation_id, class_name, rect, enabled, has_keyboard_focus, is_keyboard_focusable, toggle_state, patterns, children, omitted_children. truncated: true when the read left anything out (a budget, the depth, a provider failing partway).",
        annotations(read_only_hint = true)
    )]
    async fn accessibility_tree(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<TreeParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (target, depth) = valid!(p.validate());
        let read = self
            .worker
            .run(&ct, move |d| {
                d.tree(target, depth, Instant::now() + READ_DEADLINE)
            })
            .await;
        respond(read.map(|r| json!({ "roots": r.roots, "truncated": r.truncated })))
    }

    #[tool(
        description = "Find elements in a window or process by name (exact), name_contains (case-insensitive), role (control type, e.g. Button) and/or automation_id; all given criteria must match. Returns a flat list of nodes without children, and truncated: true when the read did not see the whole tree (then no match is not proof of absence).",
        annotations(read_only_hint = true)
    )]
    async fn find(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<FindParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (target, query) = valid!(p.validate());
        let found = self
            .worker
            .run(&ct, move |d| {
                d.find(target, &query, Instant::now() + READ_DEADLINE)
            })
            .await;
        respond(
            found.map(|(matches, read)| json!({ "matches": matches, "truncated": read.truncated })),
        )
    }

    #[tool(
        description = "Wait until an element matching the criteria (as for find) exists, polling every 250 ms. Returns the first match, or a timeout error listing the last tree seen. With pid, windows that appear or close meanwhile are followed; with window_id, a window that closes ends the wait. The reply can come later than timeout_ms by one read, and by however long other calls hold the desktop thread."
    )]
    async fn wait_for(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<WaitForParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (target, query, timeout) = valid!(p.validate());
        let deadline = Instant::now() + timeout;
        let mut summary = String::from("(no tree read: the target had no window)");
        loop {
            let q = query.clone();
            match self
                .worker
                .run(&ct, move |d| {
                    // Counted from when the read starts, not when it queued,
                    // and never so short that a poll the queue delayed past
                    // the wait's end reads nothing at all.
                    let now = Instant::now();
                    let read_deadline = deadline.max(now + MIN_READ).min(now + READ_DEADLINE);
                    d.find(target, &q, read_deadline)
                })
                .await
            {
                Ok((matches, read)) => {
                    if let Some(first) = matches.into_iter().next() {
                        return element_reply(Ok(first));
                    }
                    summary = a11y::summarize(&read.roots, 60);
                    if read.truncated {
                        summary
                            .push_str("\n(the read was truncated: it did not see the whole tree)");
                    }
                }
                // A pid with no window yet, or whose window closed while it
                // was read (a splash screen), is worth waiting for.
                Err(ToolError::NotFound(_) | ToolError::StaleElement(_))
                    if matches!(target, Target::Pid(_)) => {}
                // Passing: a full queue, a window that changed hands mid-read.
                Err(ToolError::Busy(_)) => {}
                Err(e) => return respond::<Value>(Err(e)),
            }
            if Instant::now() >= deadline {
                return respond::<Value>(Err(ToolError::Timeout {
                    timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    what: query.describe(),
                    summary,
                }));
            }
            if pause(
                &ct,
                POLL.min(deadline.saturating_duration_since(Instant::now())),
            )
            .await
            {
                return respond::<Value>(Err(ToolError::Cancelled));
            }
        }
    }

    #[tool(
        description = "Invoke (press) an element through its Invoke pattern. No pointer involved; works even when the window is covered. An error that says the action may have run means: read the tree before retrying."
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
        description = "Flip a checkbox/switch through its Toggle pattern. Returns the element with its new toggle_state."
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
        description = "Replace an element's value (at most 100000 characters) through its Value pattern (text fields), or RangeValue (sliders; value must be a number). Returns the element afterwards."
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
        description = "Give an element keyboard focus through the accessibility API. Succeeds once keyboard focus is on the element or inside it; an element that accepts the request but keeps no focus is an error."
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
        description = "Select an item (list item, tab, radio button) through its SelectionItem pattern."
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
        description = "Real mouse click at an element's clickable point (element) or a screen point (x, y). Pass window_id or pid: the click is refused unless that window is in front and holds the point. button: left|right|middle (logical: swapped buttons are honoured); double for a double-click."
    )]
    async fn click(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ClickParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (at, target) = valid!(p.validate());
        let (button, double) = (p.button.into(), p.double);
        respond(
            self.worker
                .run(&ct, move |d| d.click(&at, button, double, target))
                .await,
        )
    }

    #[tool(
        description = "Move the pointer to a screen point (screen coordinates: physical pixels on Windows, points on macOS). Returns where the pointer ended up. Sends no press, so it takes no safety target; refused while a button is still held from a failed release."
    )]
    async fn move_mouse(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<MoveMouseParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        respond(self.worker.run(&ct, move |d| d.move_mouse(p.x, p.y)).await)
    }

    #[tool(
        description = "Left-button drag from one screen point to another over duration_ms. Pass window_id or pid: refused unless that window is in front and holds both points and every point between. If that stops holding partway, the button is released at a point verified inside the target where one still verifies; otherwise the release, the one event that must go out, happens where the pointer is, and the error says so."
    )]
    async fn drag(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<DragParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (target, duration) = valid!(p.validate());
        let (from, to) = ((p.from.x, p.from.y), (p.to.x, p.to.y));
        respond(
            self.worker
                .run(&ct, move |d| d.drag(from, to, duration, target))
                .await,
        )
    }

    #[tool(
        description = "Scroll the wheel at a screen point: dy notches (positive = down), dx notches (positive = right), at most 100 each. Pass window_id or pid: refused unless that window is in front and holds the point."
    )]
    async fn scroll(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ScrollParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let target = valid!(p.validate());
        respond(
            self.worker
                .run(&ct, move |d| d.scroll(p.x, p.y, p.dx, p.dy, target))
                .await,
        )
    }

    #[tool(
        description = "Type text (at most 10000 characters) into the focused control as real keystrokes, layout-independent; a newline is Enter, a tab is Tab. Pass window_id or pid: refused unless that window is in front and holds keyboard focus in its own process. If it stops partway, the error says how many characters went out."
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
                .await,
        )
    }

    #[tool(
        description = "Press a key or combo as real keystrokes: enter, tab, esc, f5, ctrl+shift+s, alt+f4, cmd+q, ctrl+plus (modifiers: ctrl shift alt meta/win/cmd; on Windows a character that needs Shift or AltGr on the layout gets it). repeat presses it N times. Pass window_id or pid: refused unless that window is in front and holds keyboard focus in its own process; shell hotkeys are refused then."
    )]
    async fn key(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<KeyParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let (combo, repeat, target) = valid!(p.validate());
        respond(
            self.worker
                .run(&ct, move |d| d.key(&combo, repeat, target))
                .await,
        )
    }

    #[tool(
        description = "Bring a window (window_id) or a process's window (pid) to the foreground, restoring it if minimized. Reports became_foreground: Windows can refuse focus changes, so check it before sending input. Windows only for now."
    )]
    async fn activate_window(
        &self,
        ct: CancellationToken,
        Parameters(Args(p)): Parameters<Args<ActivateParams>>,
    ) -> CallToolResult {
        let p = valid!(p);
        let target = valid!(required_target(p.window_id, p.pid));
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
    use super::*;

    #[test]
    fn process_threads_are_admitted_up_to_the_bound() {
        let held: Vec<ProcessSlot> = (0..LAUNCHES.limit)
            .map(|_| ProcessSlot::take(&LAUNCHES).expect("BUG: below the bound"))
            .collect();
        // Not `Busy`: slots held by stuck spawns do not free up by retrying.
        assert!(matches!(
            ProcessSlot::take(&LAUNCHES),
            Err(ToolError::NotSupported(_))
        ));
        // Kills have threads of their own while launches are stuck.
        assert!(ProcessSlot::take(&KILLS).is_ok());
        drop(held);
        assert!(ProcessSlot::take(&LAUNCHES).is_ok());
    }
}
