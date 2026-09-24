//! The desktop session: accessibility backend, input device and window
//! lookups, owned by the worker thread and called one request at a time.
//!
//! Every input method that takes a safety target checks it immediately
//! before sending the input: the target must own the foreground window, and a
//! coordinate must be where the OS says the target's window is, uncovered.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::a11y::{self, AccessibilityBackend, Action, Node, Query, Read};
use crate::capture::{self, Shot, ShotTarget, WindowInfo};
use crate::error::{ToolError, ToolResult};
use crate::geometry::Rect;
use crate::input::{Input, MouseButton};
use crate::keys::KeyCombo;
use crate::os::{self, Focus, Under};
use crate::params::{ClickAt, ScreenshotTarget, Target};

/// The foreground window as the safety check sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Foreground {
    /// Window id.
    pub id: u32,
    /// Owning process.
    pub pid: u32,
    /// Title, when known.
    pub title: Option<String>,
    /// Bounds, when known.
    pub rect: Option<Rect>,
}

impl std::fmt::Display for Foreground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "window {} of process {}", self.id, self.pid)?;
        if let Some(t) = &self.title {
            write!(f, " {t:?}")?;
        }
        Ok(())
    }
}

fn not_foreground(target: impl std::fmt::Display, fg: Option<&Foreground>) -> ToolError {
    ToolError::NotForeground {
        target: target.to_string(),
        foreground: fg.map_or_else(|| "none".to_owned(), ToString::to_string),
    }
}

/// Fail closed: a point is only sent where the OS confirms what is under it.
fn under_or_refuse((x, y): (i32, i32), under: Option<Under>) -> ToolResult<Under> {
    under.ok_or_else(|| {
        ToolError::NotSupported(format!(
            "cannot tell what is under ({x}, {y}) (it is off every screen, or this OS does not report it), so no input is sent there"
        ))
    })
}

fn covered((x, y): (i32, i32), what: &str, under: Under) -> ToolError {
    let by = if under.inner_pid == under.pid {
        format!("window {} of process {}", under.id, under.pid)
    } else {
        format!(
            "a window of process {} inside window {} of process {}",
            under.inner_pid, under.id, under.pid
        )
    };
    ToolError::OutsideTarget {
        x,
        y,
        reason: format!("{what}, and the point is covered by {by}"),
    }
}

/// Refuses input unless `target` owns the foreground window and, at every
/// point, the OS reports the target there. `under` gives, per point, what
/// the OS says is there (`None` where it cannot say). A window target admits
/// only that window at a point; a process target any of its windows (its own
/// popups). Either way the deepest window at the point, the one a click
/// reaches, must belong to the same process.
pub fn verify(
    target: Target,
    foreground: Option<&Foreground>,
    points: &[((i32, i32), Option<Under>)],
) -> ToolResult<()> {
    let owns = foreground.is_some_and(|fg| match target {
        Target::Window(id) => fg.id == id,
        Target::Pid(pid) => fg.pid == pid,
    });
    if !owns {
        return Err(not_foreground(target, foreground));
    }
    for &(point, under) in points {
        let under = under_or_refuse(point, under)?;
        let admitted = under.inner_pid == under.pid
            && match target {
                Target::Window(id) => under.id == id,
                Target::Pid(pid) => under.pid == pid,
            };
        if !admitted {
            return Err(covered(point, &format!("the target is {target}"), under));
        }
    }
    Ok(())
}

/// Refuses keyboard input unless it reaches the foreground window's own
/// process: keystrokes go to the window holding keyboard focus, which can be
/// another process's child window inside it (an embedded browser, a preview
/// pane), and where the OS cannot say which window that is, nothing that
/// passed [`verify`] can be trusted to receive them.
pub fn verify_focus(foreground: Option<&Foreground>, focus: Focus) -> ToolResult<()> {
    let Some(fg) = foreground else {
        return Err(not_foreground("the target", None));
    };
    match focus {
        Focus::Foreground => Ok(()),
        Focus::Pid(pid) if pid == fg.pid => Ok(()),
        Focus::Pid(pid) => Err(ToolError::NotForeground {
            target: format!("window {} of process {}", fg.id, fg.pid),
            foreground: format!(
                "{fg}, but keyboard focus is inside it in a window of process {pid}"
            ),
        }),
        Focus::Unknown => Err(ToolError::NotSupported(format!(
            "this OS does not report which window has keyboard focus ({fg} could hold it inside another process's panel), so keys and text with a safety target are refused"
        ))),
    }
}

/// Refuses a click on an element unless the element's own top-level window
/// (`window`, of process `pid`) is what the OS reports at the point and its
/// process owns the foreground. Its window itself need not be in front: a
/// popup menu or a drop-down never is, while its application is.
pub fn verify_element(
    window: u32,
    pid: u32,
    foreground: Option<&Foreground>,
    point: (i32, i32),
    under: Option<Under>,
) -> ToolResult<()> {
    if foreground.is_none_or(|fg| fg.pid != pid) {
        return Err(not_foreground(
            format!("process {pid}, which owns the element"),
            foreground,
        ));
    }
    let under = under_or_refuse(point, under)?;
    if under.id != window || under.inner_pid != pid {
        return Err(covered(
            point,
            &format!("the element is in window {window}"),
            under,
        ));
    }
    Ok(())
}

/// Refuses a window target whose id now belongs to a different process than
/// the one that owned it when this session listed it (`bound`): Windows
/// recycles `HWND`s, and input must not follow an id to an unrelated window.
pub fn still_bound(target: Target, bound: Option<u32>, fg: Option<&Foreground>) -> ToolResult<()> {
    if let (Target::Window(id), Some(pid), Some(fg)) = (target, bound, fg)
        && fg.id == id
        && fg.pid != pid
    {
        return Err(ToolError::NotForeground {
            target: format!("{target} (of process {pid} when listed)"),
            foreground: format!("{fg}: the id now belongs to another process"),
        });
    }
    Ok(())
}

/// Refuses a pid whose process is not the one this session first saw under
/// it (`then`, its start time): OSes recycle pids, and input must not follow
/// a pid to an unrelated process. `now` is the start time of the process
/// holding the pid now, `None` when none does or the OS cannot say. Without
/// a start time there is no identity to hold the pid to, so it is refused.
pub fn same_process(pid: u32, then: Option<u64>, now: Option<u64>) -> ToolResult<()> {
    match then {
        None => Err(ToolError::NotSupported(format!(
            "process {pid} cannot be told apart from a later process reusing its pid (this OS reports no start time), so it is not a safety target; pass window_id"
        ))),
        // Not `NotFound`: a `wait_for` keeps polling on that, and a reused
        // pid never turns back into the process it named.
        Some(then) if now != Some(then) => Err(ToolError::InvalidArgument(format!(
            "process {pid} has exited since this session saw it{}; this session does not re-bind a pid it handed out, so target the new process by window_id",
            if now.is_some() {
                " (the pid now belongs to another process)"
            } else {
                ""
            }
        ))),
        _ => Ok(()),
    }
}

/// What a target is held to for one call: the process a window id was
/// issued for, and the start time of the process the window's owner or the
/// pid named when issued. Every event's check compares against it again, so
/// a target that exits and has its number reused mid-action (a long drag,
/// typed text) stops receiving.
#[derive(Debug, Clone, Copy, Default)]
struct Binding {
    window_pid: Option<u32>,
    started: Option<u64>,
    /// The window's class fingerprint when issued ([`os::window_class`]).
    class: Option<u64>,
}

/// What a window id was bound to when this session handed it out.
#[derive(Debug, Clone, Copy)]
struct Issued {
    pid: u32,
    started: Option<u64>,
    class: Option<u64>,
}

/// Set once the server is shutting down: every input action checks it
/// before each event (it runs its checks even without a safety target) and
/// stops, so the release of held input that follows is never raced by a
/// long `type_text` or drag.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// Makes every input action in progress stop at its next event, and every
/// queued desktop call be skipped.
pub fn stop_input() {
    STOPPING.store(true, Ordering::SeqCst);
}

/// Whether shutdown has begun.
pub fn stopping() -> bool {
    STOPPING.load(Ordering::SeqCst)
}

/// Session state on the worker thread.
pub struct Desktop {
    a11y: Box<dyn AccessibilityBackend>,
    input: Result<Input, String>,
    /// Every window id this session handed out, with the process that owned
    /// it then and that process's start time (`None` where the OS reports
    /// none). Windows recycles an `HWND`, and a pid, for an unrelated window,
    /// so a window target is accepted only while that same process owns it.
    issued: HashMap<u32, Issued>,
    /// Every pid this session handed out (listed or launched), with its
    /// process's start time: a pid target is accepted only while that
    /// process holds the pid. A pid whose start time the OS cannot report is
    /// never recorded, so it is never accepted as a target.
    started: HashMap<u32, u64>,
}

impl std::fmt::Debug for Desktop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Desktop")
            .field("input", &self.input.is_ok())
            .finish_non_exhaustive()
    }
}

impl Desktop {
    /// Opens the accessibility backend and the input device on this thread.
    /// Either may be unavailable; the tools that need it then report why.
    pub fn new() -> Self {
        Self {
            a11y: a11y::backend(),
            input: Input::new(),
            issued: HashMap::new(),
            started: HashMap::new(),
        }
    }

    fn input(&mut self) -> ToolResult<&mut Input> {
        self.input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))
    }

    /// Releases every button and key this session's input may hold: after a
    /// job panicked mid-action, and before the server exits.
    pub fn release_input(&mut self) {
        if let Ok(input) = &mut self.input {
            input.release_all();
        }
    }

    /// Binds `pid` to the process `launch` just started under it. The one
    /// re-binding there is: this session created that process and hands the
    /// pid out now, so it is the sighting the caller holds. Window ids stay
    /// bound to the start time recorded with each, so none of them follows.
    /// `started` is read at the spawn, while the launcher still holds the
    /// child, so it cannot belong to a later process reusing the pid.
    ///
    /// A pid this session already handed out for another process is not
    /// re-bound: a target still holding the old one would come to name the
    /// new process. Whether the pid is bound comes back.
    pub fn bind_launched(&mut self, pid: u32, started: Option<u64>) -> bool {
        let Some(started) = started else {
            return false;
        };
        if let Some(&then) = self.started.get(&pid) {
            return then == started;
        }
        self.started.insert(pid, started);
        true
    }

    /// Records a window id with its owner and the owner's start time, unless
    /// the id is already bound.
    ///
    /// The start time is read after the window list was taken, so it is kept
    /// only if the window still belongs to that pid afterwards: a window
    /// cannot outlive its process, so its owner was alive, under that pid,
    /// when the start time was read. A window that went meanwhile is not
    /// bound at all (its id is refused as never listed).
    fn adopt_window(&mut self, w: &WindowInfo) {
        let started = os::process_started(w.pid);
        if cfg!(target_os = "windows") && os::window_pid(w.id) != Some(w.pid) {
            return;
        }
        self.issued.entry(w.id).or_insert(Issued {
            pid: w.pid,
            started,
            class: os::window_class(w.id),
        });
        if let Some(started) = started {
            self.started.entry(w.pid).or_insert(started);
        }
    }

    /// The windows of the process `launch` started as `pid` (at `started`),
    /// or `None` once that process is gone: the pid may already name
    /// another process, whose windows are neither returned nor bound.
    pub fn launched_windows(
        &mut self,
        pid: u32,
        started: Option<u64>,
    ) -> ToolResult<Option<Vec<WindowInfo>>> {
        // Without a start time nothing tells the launched process from a
        // later one under its pid, so no window is taken as its.
        let Some(started) = started else {
            return Ok(None);
        };
        let same = || os::process_started(pid) == Some(started);
        if !same() {
            return Ok(None);
        }
        let windows: Vec<WindowInfo> = capture::windows()?
            .into_iter()
            .filter(|w| w.pid == pid)
            .collect();
        if !same() {
            return Ok(None);
        }
        for w in &windows {
            self.adopt_window(w);
        }
        Ok(Some(windows))
    }

    /// The window list, optionally filtered. Every id and pid it returns is
    /// recorded with its owner the first time it is seen, so a later target
    /// can be bound to it.
    pub fn list_windows(
        &mut self,
        title_contains: Option<&str>,
        pid: Option<u32>,
    ) -> ToolResult<Vec<WindowInfo>> {
        let needle = title_contains.map(a11y::fold);
        let windows: Vec<WindowInfo> = capture::windows()?
            .into_iter()
            .filter(|w| pid.is_none_or(|p| w.pid == p))
            .filter(|w| {
                needle
                    .as_deref()
                    .is_none_or(|n| a11y::fold(&w.title).contains(n))
            })
            .collect();
        // A listing never re-binds: an agent may still hold an id or pid from
        // an earlier listing, and a later one seeing the number reused must
        // not make that old target name the new process.
        for w in &windows {
            self.adopt_window(w);
        }
        Ok(windows)
    }

    /// What a target is bound to: for a window id, the owner and start time
    /// recorded when this session handed it out; for a pid, the start time
    /// of the process it named then. A number never handed out is refused —
    /// it cannot be told apart from a recycled one.
    fn bound(&mut self, target: Option<Target>) -> ToolResult<Binding> {
        match target {
            Some(Target::Window(id)) => {
                let &Issued {
                    pid,
                    started,
                    class,
                } = self.issued.get(&id).ok_or_else(|| {
                    ToolError::InvalidArgument(format!(
                        "window {id} was not listed in this session; take window ids from list_windows or launch"
                    ))
                })?;
                Ok(Binding {
                    window_pid: Some(pid),
                    started,
                    class,
                })
            }
            Some(Target::Pid(pid)) => {
                let now = os::process_started(pid);
                let Some(&then) = self.started.get(&pid) else {
                    if now.is_none() && !cfg!(target_os = "windows") {
                        // The OS has no start times: the reason to report.
                        same_process(pid, None, None)?;
                    }
                    // Not `NotFound`: `wait_for` polls on that, and an unissued
                    // pid never becomes issued by waiting.
                    return Err(ToolError::InvalidArgument(format!(
                        "process {pid} was not listed or launched in this session; take pids from list_windows or launch"
                    )));
                };
                same_process(pid, Some(then), now)?;
                Ok(Binding {
                    window_pid: None,
                    started: Some(then),
                    class: None,
                })
            }
            None => Ok(Binding::default()),
        }
    }

    /// The windows a target names: one window, or a process's windows front
    /// to back.
    pub fn resolve(target: Target) -> ToolResult<Vec<WindowInfo>> {
        let all = capture::windows()?;
        let found: Vec<_> = match target {
            Target::Window(id) => all.into_iter().filter(|w| w.id == id).collect(),
            Target::Pid(pid) => all.into_iter().filter(|w| w.pid == pid).collect(),
        };
        if found.is_empty() {
            return Err(no_window(target));
        }
        Ok(found)
    }

    /// Captures a window, a process's frontmost window, or a monitor.
    ///
    /// A window or pid is held to the identity it was issued with, before the
    /// capture and again after it: pixels of an application that took over a
    /// recycled id are refused, not returned.
    pub fn screenshot(
        &mut self,
        target: ScreenshotTarget,
        max_side: Option<u32>,
    ) -> ToolResult<Shot> {
        capture::available()?;
        let (direct, held) = match target {
            ScreenshotTarget::Direct(ShotTarget::Window(id)) => {
                let bound = self.bound(Some(Target::Window(id)))?;
                (ShotTarget::Window(id), Some((Target::Window(id), bound)))
            }
            ScreenshotTarget::Direct(t) => (t, None),
            ScreenshotTarget::Pid(pid) => {
                // Held to its identity like any other target: without one
                // (macOS) a reused pid would capture another application's
                // window, so the pid is refused and window_id is the way.
                let started = self.bound(Some(Target::Pid(pid)))?.started;
                let windows = Self::resolve(Target::Pid(pid))?;
                let pick = windows
                    .iter()
                    .find(|w| w.is_focused)
                    .or_else(|| windows.iter().find(|w| !w.is_minimized))
                    .unwrap_or(&windows[0]);
                let bound = Binding {
                    window_pid: Some(pid),
                    started,
                    class: os::window_class(pick.id),
                };
                (
                    ShotTarget::Window(pick.id),
                    Some((Target::Window(pick.id), bound)),
                )
            }
        };
        let recheck = || held.map_or(Ok(()), |(t, b)| Self::revalidate(t, b));
        recheck()?;
        let shot = capture::screenshot(direct, max_side)?;
        recheck()?;
        Ok(shot)
    }

    /// The element trees of a target's windows. A process's popups (menus,
    /// drop-downs) are windows of their own and are read too where the OS
    /// lists them.
    ///
    /// The target is held to the identity it was issued with, as for input:
    /// a recycled window id or pid would otherwise hand out element handles
    /// in an unrelated application, which a later `set_value` or `invoke`
    /// (which take no target) would then act on.
    pub fn tree(
        &mut self,
        target: Target,
        max_depth: usize,
        deadline: Instant,
    ) -> ToolResult<Read> {
        self.a11y.available()?;
        let bound = self.bound(Some(target))?;
        Self::revalidate(target, bound)?;
        let ids: Vec<u32> = match target {
            Target::Pid(pid) => match os::process_windows(pid)? {
                Some(ids) if !ids.is_empty() => ids,
                Some(_) => return Err(no_window(target)),
                None => Self::resolve(target)?.iter().map(|w| w.id).collect(),
            },
            Target::Window(_) => Self::resolve(target)?.iter().map(|w| w.id).collect(),
        };
        let read = self.a11y.tree(&ids, max_depth, deadline)?;
        // Checked again after the read: a window or process recycled while
        // it was read would otherwise hand out handles in another
        // application, which `invoke` and `set_value` (no target) act on.
        Self::revalidate(target, bound)?;
        let owner = match target {
            Target::Pid(pid) => Some(pid),
            Target::Window(_) => bound.window_pid,
        };
        if let Some(pid) = owner
            && let Some((id, now)) = ids.iter().find_map(|&id| {
                os::window_pid(id)
                    .filter(|&now| now != pid)
                    .map(|now| (id, now))
            })
        {
            return Err(ToolError::InvalidArgument(format!(
                "window {id} changed owner to process {now} while it was read; read again"
            )));
        }
        Ok(read)
    }

    /// Elements matching `query` anywhere in a target's windows, and the
    /// read they came from.
    pub fn find(
        &mut self,
        target: Target,
        query: &Query,
        deadline: Instant,
    ) -> ToolResult<(Vec<Node>, Read)> {
        // Bounded like `accessibility_tree`: an unbounded walk of a deeply
        // nested tree (a browser's, a document's) recurses until the worker's
        // stack runs out.
        let read = self.tree(target, crate::params::MAX_DEPTH as usize, deadline)?;
        Ok((a11y::search(&read.roots, query), read))
    }

    /// Performs a pattern action.
    pub fn act(&mut self, element: &str, action: &Action) -> ToolResult<Node> {
        self.a11y.act(element, action)
    }

    /// The foreground window, read straight from the OS where it can be, so
    /// the check before every input event stays cheap.
    fn foreground() -> ToolResult<Option<Foreground>> {
        if let Some((id, pid)) = os::foreground()
            && let Some(rect) = os::window_rect(id)
        {
            return Ok(Some(Foreground {
                id,
                pid,
                title: os::window_title(id),
                rect: Some(rect),
            }));
        }
        let windows = capture::windows()?;
        Ok(match os::foreground() {
            Some((id, pid)) => {
                let listed = windows.iter().find(|w| w.id == id);
                Some(Foreground {
                    id,
                    pid,
                    title: listed.map(|w| w.title.clone()),
                    rect: listed.map(|w| w.rect),
                })
            }
            None => windows.iter().find(|w| w.is_focused).map(|w| Foreground {
                id: w.id,
                pid: w.pid,
                title: Some(w.title.clone()),
                rect: Some(w.rect),
            }),
        })
    }

    /// The process that owns window `id` now: from the OS where it answers
    /// directly, else from the window list (macOS), so an owner check never
    /// passes just because the fast lookup is missing.
    fn owner_now(id: u32) -> Option<u32> {
        os::window_pid(id).or_else(|| {
            capture::windows()
                .ok()?
                .into_iter()
                .find(|w| w.id == id)
                .map(|w| w.pid)
        })
    }

    /// Refuses a target that no longer names what it was issued for: a
    /// window whose owner changed, or a process (the pid's, or the window
    /// owner's) whose start time changed — Windows can recycle both the
    /// `HWND` and the pid of an application that exited.
    fn revalidate(target: Target, bound: Binding) -> ToolResult<()> {
        let pid = match target {
            Target::Pid(pid) => Some(pid),
            Target::Window(id) => {
                if let (Some(then), Some(now)) = (bound.class, os::window_class(id))
                    && now != then
                {
                    return Err(ToolError::InvalidArgument(format!(
                        "window {id} is no longer the window this session listed (its class changed); list_windows again"
                    )));
                }
                if let Some(pid) = bound.window_pid {
                    match Self::owner_now(id) {
                        // Gone: nothing read or sent can be about it any more.
                        None => return Err(no_window(target)),
                        Some(now) if now != pid => {
                            return Err(ToolError::InvalidArgument(format!(
                                "window {id} belonged to process {pid} when listed and now belongs to process {now}; this session does not re-bind it, so target the new window's process by pid"
                            )));
                        }
                        Some(_) => {}
                    }
                }
                bound.window_pid
            }
        };
        if let (Some(pid), Some(then)) = (pid, bound.started) {
            same_process(pid, Some(then), os::process_started(pid))?;
        }
        Ok(())
    }

    fn check(target: Option<Target>, bound: Binding, points: &[(i32, i32)]) -> ToolResult<()> {
        // Shutting down: the action in progress stops at its next event and
        // releases what it holds, instead of the server exiting under it.
        if STOPPING.load(Ordering::SeqCst) {
            return Err(ToolError::Cancelled);
        }
        let Some(target) = target else {
            return Ok(());
        };
        Self::revalidate(target, bound)?;
        let fg = Self::foreground()?;
        still_bound(target, bound.window_pid, fg.as_ref())?;
        if points.is_empty() {
            // Keys and text: they go to the focused window, which must be
            // the target's process too; the focus is read on the very
            // window just checked.
            verify(target, fg.as_ref(), &[])?;
            let focus = match &fg {
                Some(fg) => os::focus(fg.id)?,
                None => Focus::Unknown,
            };
            return verify_focus(fg.as_ref(), focus);
        }
        let points: Vec<_> = points
            .iter()
            .map(|&(x, y)| ((x, y), os::window_at(x, y)))
            .collect();
        verify(target, fg.as_ref(), &points)
    }

    fn check_element(window: u32, pid: u32, (x, y): (i32, i32)) -> ToolResult<()> {
        let fg = Self::foreground()?;
        verify_element(window, pid, fg.as_ref(), (x, y), os::window_at(x, y))
    }

    /// Clicks an element's clickable point or a screen point.
    pub fn click(
        &mut self,
        at: &ClickAt,
        button: MouseButton,
        double: bool,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        self.input()?;
        let bound = self.bound(target)?;
        let (x, y, element_window) = match at {
            ClickAt::Point(x, y) => (*x, *y, None),
            ClickAt::Element(handle) => {
                let p = self.a11y.click_point(handle)?;
                // The element's own top-level window: a sibling window of the
                // same process in front would take a click checked against the
                // process alone, so an unknown window is refused, not relaxed.
                let window = p.window.ok_or_else(|| {
                    ToolError::NotFound(format!(
                        "cannot tell which window element `{handle}` belongs to, so it is not clicked; use invoke"
                    ))
                })?;
                let pid = os::window_pid(window)
                    .ok_or_else(|| ToolError::StaleElement(handle.clone()))?;
                (p.x, p.y, Some((handle.as_str(), window, pid)))
            }
        };
        let Self { a11y, input, .. } = self;
        let input = input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))?;
        let mut guard = |_: Option<(i32, i32)>| {
            // An element click always lands on the element. The slow check
            // (a UI Automation hit-test, a cross-process call) runs first and
            // the fast OS checks last, right before the event, so what they
            // saw is as fresh as it can be.
            if let Some((handle, window, pid)) = element_window {
                if !a11y.hits(handle, x, y)? {
                    return Err(ToolError::OutsideTarget {
                        x,
                        y,
                        reason: format!(
                            "element `{handle}` is no longer what is under it (something inside its window covers it); use invoke, or read the tree again"
                        ),
                    });
                }
                Self::check_element(window, pid, (x, y))?;
            }
            Self::check(target, bound, &[(x, y)])
        };
        guard(None)?;
        input.click(x, y, button, double, &mut guard)?;
        Ok(
            json!({ "clicked": { "x": x, "y": y }, "button": format!("{button:?}").to_lowercase(), "double": double }),
        )
    }

    /// Moves the pointer.
    pub fn move_mouse(&mut self, x: i32, y: i32) -> ToolResult<Value> {
        // Shutting down: no pointer moves once the session has ended.
        if STOPPING.load(Ordering::SeqCst) {
            return Err(ToolError::Cancelled);
        }
        let input = self.input()?;
        // A move with a button still held from a failed release is a drag.
        input.ready()?;
        input.move_to(x, y)?;
        std::thread::sleep(Duration::from_millis(10));
        let at = input.position();
        Ok(
            json!({ "requested": { "x": x, "y": y }, "pointer": at.map(|(x, y)| json!({ "x": x, "y": y })) }),
        )
    }

    /// Drags with the left button. Each step checks the point it is about to
    /// reach; the start and end are checked before the press.
    pub fn drag(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        duration: Duration,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        self.input()?;
        let bound = self.bound(target)?;
        Self::check(target, bound, &[from, to])?;
        let mut guard = |at: Option<(i32, i32)>| match at {
            Some(point) => Self::check(target, bound, &[point]),
            None => Self::check(target, bound, &[from, to]),
        };
        self.input()?.drag(from, to, duration, &mut guard)?;
        Ok(json!({ "from": { "x": from.0, "y": from.1 }, "to": { "x": to.0, "y": to.1 } }))
    }

    /// Scrolls at a point.
    pub fn scroll(
        &mut self,
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        self.input()?;
        let bound = self.bound(target)?;
        Self::check(target, bound, &[(x, y)])?;
        let mut guard = |_: Option<(i32, i32)>| Self::check(target, bound, &[(x, y)]);
        self.input()?.scroll(x, y, dx, dy, &mut guard)?;
        Ok(json!({ "at": { "x": x, "y": y }, "dx": dx, "dy": dy }))
    }

    /// Types text into whatever has keyboard focus.
    pub fn type_text(&mut self, text: &str, target: Option<Target>) -> ToolResult<Value> {
        self.input()?;
        let bound = self.bound(target)?;
        Self::check(target, bound, &[])?;
        let mut guard = |_: Option<(i32, i32)>| Self::check(target, bound, &[]);
        self.input()?.type_text(text, &mut guard)?;
        Ok(json!({ "typed_chars": text.chars().count() }))
    }

    /// Presses a key combo.
    pub fn key(
        &mut self,
        combo: &KeyCombo,
        repeat: u32,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        self.input()?;
        if target.is_some()
            && let Some(handler) = combo.shell_hotkey(cfg!(target_os = "macos"), repeat)
        {
            return Err(ToolError::InvalidArgument(format!(
                "`{combo}` is handled by {handler}, not by the target window, so no safety target can hold for it; it is refused"
            )));
        }
        let bound = self.bound(target)?;
        Self::check(target, bound, &[])?;
        let mut guard = |_: Option<(i32, i32)>| Self::check(target, bound, &[]);
        self.input()?.key(combo, repeat, &mut guard)?;
        Ok(json!({ "pressed": combo.to_string(), "repeat": repeat }))
    }

    /// Brings a window to the front and reports whether it got there. A
    /// window id must be one this session listed, still owned by the same
    /// process; activating never re-binds an id.
    pub fn activate(&mut self, target: Target) -> ToolResult<Value> {
        if !cfg!(target_os = "windows") {
            return Err(ToolError::NotSupported(format!(
                "activate_window is not supported on {} yet",
                std::env::consts::OS
            )));
        }
        let bound = self.bound(Some(target))?;
        Self::revalidate(target, bound)?;
        let windows = Self::resolve(target)?;
        for w in &windows {
            self.adopt_window(w);
        }
        let window = windows
            .iter()
            .find(|w| !w.is_minimized)
            .unwrap_or(&windows[0])
            .clone();
        let mut attempts = Vec::new();
        // The chosen window itself, not only its process, is what gets
        // raised: held to its owner, the process's start time and its class
        // as resolved, right before each attempt.
        let chosen = Binding {
            window_pid: Some(window.pid),
            started: bound
                .started
                .or_else(|| self.issued.get(&window.id).and_then(|i| i.started)),
            class: os::window_class(window.id),
        };
        let recheck = || {
            Self::revalidate(target, bound)?;
            Self::revalidate(Target::Window(window.id), chosen)
        };
        recheck()?;
        os::bring_to_front(window.id)?;
        attempts.push("SetForegroundWindow");
        let mut fg = Self::foreground()?;
        if fg.as_ref().map(|f| f.id) != Some(window.id) {
            recheck()?;
            if let Err(e) = self.a11y.focus_window(window.id) {
                tracing::debug!("UIA focus fallback failed: {e}");
            }
            attempts.push("UIA SetFocus");
            std::thread::sleep(Duration::from_millis(100));
            fg = Self::foreground()?;
        }
        let became = fg.as_ref().map(|f| f.id) == Some(window.id);
        Ok(json!({
            "window_id": window.id,
            "became_foreground": became,
            "foreground": fg,
            "attempts": attempts,
        }))
    }
}

fn no_window(target: Target) -> ToolError {
    ToolError::NotFound(match target {
        Target::Window(id) => format!("no window with id {id}; call list_windows"),
        Target::Pid(pid) => format!(
            "process {pid} has no top-level window; if the app hands off to another \
             process (Windows 11 Notepad does), find it with list_windows and pass window_id"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg() -> Foreground {
        Foreground {
            id: 10,
            pid: 100,
            title: Some("App".into()),
            rect: Some(Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            }),
        }
    }

    const OWN: Under = Under {
        id: 10,
        pid: 100,
        inner_pid: 100,
    };

    #[test]
    fn refuses_when_nothing_or_something_else_is_in_front() {
        assert!(matches!(
            verify(Target::Pid(100), None, &[]),
            Err(ToolError::NotForeground { .. })
        ));
        assert!(matches!(
            verify(Target::Window(11), Some(&fg()), &[]),
            Err(ToolError::NotForeground { .. })
        ));
        assert!(matches!(
            verify(Target::Pid(101), Some(&fg()), &[]),
            Err(ToolError::NotForeground { .. })
        ));
    }

    #[test]
    fn accepts_the_foreground_target_by_window_or_pid() {
        assert!(verify(Target::Window(10), Some(&fg()), &[]).is_ok());
        assert!(verify(Target::Pid(100), Some(&fg()), &[((5, 5), Some(OWN))]).is_ok());
        assert!(verify(Target::Window(10), Some(&fg()), &[((5, 5), Some(OWN))]).is_ok());
    }

    /// A window id recycled for another process's window is refused, even
    /// though that window is in front under the same id.
    #[test]
    fn a_recycled_window_id_is_refused() {
        assert!(still_bound(Target::Window(10), Some(100), Some(&fg())).is_ok());
        assert!(matches!(
            still_bound(Target::Window(10), Some(555), Some(&fg())),
            Err(ToolError::NotForeground { .. })
        ));
        assert!(still_bound(Target::Pid(100), None, Some(&fg())).is_ok());
    }

    /// A pid held by another process than the one first seen under it, or
    /// by none, is refused; so is one whose process identity the OS cannot
    /// report, which could not be told from a reuse.
    #[test]
    fn a_recycled_pid_is_refused() {
        assert!(same_process(7, Some(1), Some(1)).is_ok());
        let err = same_process(7, Some(1), Some(2)).expect_err("BUG: another process");
        assert!(err.to_string().contains("another process"), "{err}");
        assert!(
            same_process(7, Some(1), None).is_err(),
            "the process exited"
        );

        let err = same_process(7, None, Some(2)).expect_err("BUG: no identity to hold");
        assert!(err.to_string().contains("window_id"), "{err}");
    }

    /// A point whose window the OS cannot name is refused, not waved
    /// through: on an OS without hit-testing a covering window would
    /// otherwise take the input.
    #[test]
    fn a_point_with_nothing_known_under_it_fails_closed() {
        assert!(matches!(
            verify(Target::Pid(100), Some(&fg()), &[((5, 5), None)]),
            Err(ToolError::NotSupported(_))
        ));
    }

    /// A window target admits only that window at the point: a sibling
    /// window of the same process in front of it would take the click.
    #[test]
    fn a_window_target_refuses_its_processes_other_windows() {
        let sibling = Under { id: 12, ..OWN };
        assert!(matches!(
            verify(Target::Window(10), Some(&fg()), &[((5, 5), Some(sibling))]),
            Err(ToolError::OutsideTarget { .. })
        ));
    }

    #[test]
    fn refuses_covered_points_and_admits_own_popups() {
        let covered = Under {
            id: 99,
            pid: 555,
            inner_pid: 555,
        };
        let err = verify(Target::Pid(100), Some(&fg()), &[((5, 5), Some(covered))])
            .expect_err("BUG: a covered point must be refused");
        assert!(err.to_string().contains("covered"), "{err}");
        let own_popup = Under { id: 12, ..OWN };
        assert!(verify(Target::Pid(100), Some(&fg()), &[((5, 5), Some(own_popup))]).is_ok());
    }

    /// Another process's child window inside the target (a preview pane)
    /// takes the click itself, so the point is refused for either target.
    #[test]
    fn a_hosted_window_of_another_process_is_refused() {
        let hosted = Under {
            inner_pid: 555,
            ..OWN
        };
        for target in [Target::Window(10), Target::Pid(100)] {
            let err = verify(target, Some(&fg()), &[((5, 5), Some(hosted))])
                .expect_err("BUG: a hosted window of another process takes the click");
            assert!(err.to_string().contains("process 555"), "{err}");
        }
    }

    /// Keys reach the focused child window: one of another process inside
    /// the target refuses them; focus in the target's own process, or none,
    /// admits them.
    #[test]
    fn keyboard_focus_in_another_process_is_refused() {
        let err = verify_focus(Some(&fg()), Focus::Pid(555)).expect_err("BUG: focus is elsewhere");
        assert!(err.to_string().contains("process 555"), "{err}");
        assert!(verify_focus(Some(&fg()), Focus::Pid(100)).is_ok());
        assert!(verify_focus(Some(&fg()), Focus::Foreground).is_ok());
    }

    /// Where the OS cannot say which window has keyboard focus (macOS), keys
    /// and text with a target are refused: a panel of another process can
    /// hold the focus while the target is in front.
    #[test]
    fn an_unknown_keyboard_focus_fails_closed() {
        assert!(matches!(
            verify_focus(Some(&fg()), Focus::Unknown),
            Err(ToolError::NotSupported(_))
        ));
        assert!(verify_focus(None, Focus::Foreground).is_err());
    }

    /// An unissued pid or window id is an argument error, not the
    /// `NotFound` that `wait_for` keeps polling on.
    #[cfg(target_os = "windows")]
    #[test]
    fn an_unissued_target_is_not_a_missing_window() {
        let mut desktop = Desktop::new();
        assert!(matches!(
            desktop.bound(Some(Target::Pid(std::process::id()))),
            Err(ToolError::InvalidArgument(_))
        ));
        assert!(matches!(
            desktop.bound(Some(Target::Window(123_456_789))),
            Err(ToolError::InvalidArgument(_))
        ));
    }

    /// The safety wiring end to end, on any host: a shell hotkey with a
    /// target is refused before anything else is looked at, and window ids
    /// and pids this session never handed out are refused.
    #[test]
    fn a_desktop_refuses_what_it_cannot_bind() {
        let mut desktop = Desktop::new();
        let refused = |r: ToolResult<Value>| r.expect_err("BUG: must be refused").to_string();
        if desktop.input().is_ok() {
            let combo = KeyCombo::parse("win+r").expect("BUG: parses");
            let err = refused(desktop.key(&combo, 1, Some(Target::Window(1))));
            assert!(err.contains("meta+r"), "{err}");
            let err = refused(desktop.type_text("x", Some(Target::Window(123_456_789))));
            assert!(err.contains("not listed"), "{err}");
            let err = refused(desktop.type_text("x", Some(Target::Pid(u32::MAX - 7))));
            assert!(
                err.contains("not listed") || err.contains("start time"),
                "{err}"
            );
        } else {
            // No input device on this OS: that is the reason reported, not
            // a binding error about ids that could never have been listed.
            let err = refused(desktop.type_text("x", Some(Target::Window(1))));
            assert!(err.contains("not supported"), "{err}");
        }
    }

    /// An element in a popup (a menu, a drop-down) is clicked while its
    /// application is in front, though the popup window itself never is; a
    /// window covering the popup, or another application in front, refuses.
    #[test]
    fn an_element_click_needs_its_window_under_the_point_and_its_process_in_front() {
        let menu = Under {
            id: 20,
            pid: 100,
            inner_pid: 100,
        };
        assert!(verify_element(20, 100, Some(&fg()), (5, 5), Some(menu)).is_ok());
        assert!(matches!(
            verify_element(20, 100, Some(&fg()), (5, 5), Some(OWN)),
            Err(ToolError::OutsideTarget { .. })
        ));
        let other_app = Foreground { pid: 555, ..fg() };
        assert!(matches!(
            verify_element(20, 100, Some(&other_app), (5, 5), Some(menu)),
            Err(ToolError::NotForeground { .. })
        ));
        assert!(verify_element(20, 100, Some(&fg()), (5, 5), None).is_err());
    }
}
