//! The desktop session: accessibility backend, input device, the registry
//! of handles this session has issued, and the window lookups, owned by the
//! worker thread and called one request at a time.
//!
//! Every input method that takes a safety target checks it immediately
//! before sending the input: the target must own the foreground window, and a
//! coordinate must be where the OS says the target's window is, uncovered.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::a11y::{self, AccessibilityBackend, Action, Node, Query, Read};
use crate::capture::{self, NativeWindow, Shot, ShotTarget, Untargetable, Window};
use crate::error::{Effect, HandleKind, ToolError, ToolResult, WindowRef};
use crate::geometry::Rect;
use crate::input::{Input, MouseButton};
use crate::keys::KeyCombo;
use crate::os::{self, Focus, Under};
use crate::params::{Location, Scope, ScreenshotTarget, Target, TargetArg};

/// The foreground window as the safety check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// Native window id.
    pub id: u32,
    /// Owning process.
    pub pid: u32,
    /// The session handle, when this session issued one for this window.
    pub handle: Option<String>,
    /// Title, when known.
    pub title: Option<String>,
    /// Bounds, when known.
    pub rect: Option<Rect>,
}

impl Foreground {
    /// How an error names it.
    pub fn as_ref(&self) -> WindowRef {
        WindowRef {
            window: self.handle.clone(),
            pid: self.pid,
            title: self.title.clone(),
        }
    }
}

/// Names a native window for an error: its session handle when there is
/// one, and its process and title either way.
pub type Namer<'a> = &'a dyn Fn(u32, u32) -> WindowRef;

/// A screen point.
type Point = (i32, i32);

/// The element a pointer action is aimed at: its handle, its own top-level
/// window and that window's process, which the press is checked against.
type ElementWindow = (String, u32, u32);

fn not_foreground(target: impl std::fmt::Display, fg: Option<&Foreground>) -> ToolError {
    ToolError::NotForeground {
        target: target.to_string(),
        foreground: fg.map(Foreground::as_ref),
    }
}

/// Fail closed: a point is only sent where the OS confirms what is under it.
fn under_or_refuse((x, y): (i32, i32), under: Option<Under>) -> ToolResult<Under> {
    under.ok_or_else(|| ToolError::OutsideTarget {
        x,
        y,
        reason: "nothing is known to be there (it is off every screen, or this OS does not report what is under a point)".into(),
        covered_by: None,
    })
}

fn covered((x, y): (i32, i32), what: &str, under: Under, name: Namer<'_>) -> ToolError {
    let window = name(under.id, under.pid);
    let by = if under.inner_pid == under.pid {
        window.to_string()
    } else {
        format!("a window of process {} inside {window}", under.inner_pid)
    };
    ToolError::OutsideTarget {
        x,
        y,
        reason: format!("{what}, and the point is covered by {by}"),
        covered_by: Some(window),
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
    name: Namer<'_>,
) -> ToolResult<()> {
    let owns = foreground.is_some_and(|fg| match target {
        Target::Window(id, _) => fg.id == id,
        Target::Pid(pid) => fg.pid == pid,
    });
    if !owns {
        return Err(not_foreground(target, foreground));
    }
    for &(point, under) in points {
        let under = under_or_refuse(point, under)?;
        let admitted = under.inner_pid == under.pid
            && match target {
                Target::Window(id, _) => under.id == id,
                Target::Pid(pid) => under.pid == pid,
            };
        if !admitted {
            return Err(covered(
                point,
                &format!("the target is {target}"),
                under,
                name,
            ));
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
        Focus::Pid(pid) => Err(ToolError::FocusElsewhere {
            target: fg.as_ref().to_string(),
            holder: pid,
        }),
        Focus::Unknown => Err(ToolError::NotSupported(format!(
            "this OS does not report which window has keyboard focus ({} could hold it inside another process's panel), so keys and text with a safety target are refused",
            fg.as_ref()
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
    name: Namer<'_>,
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
            &format!("the element is in {}", name(window, pid)),
            under,
            name,
        ));
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
            "process {pid} cannot be told apart from a later process reusing its pid (this OS reports no start time), so it is not a safety target; pass window"
        ))),
        Some(then) if now != Some(then) => Err(ToolError::Gone {
            handle: pid.to_string(),
            kind: HandleKind::Process,
            why: if now.is_some() {
                "it has exited, and the pid now belongs to another process".into()
            } else {
                "it has exited".into()
            },
        }),
        _ => Ok(()),
    }
}

/// What a target is held to for one call: the process a window handle was
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

/// What a window handle was bound to when this session handed it out.
#[derive(Debug, Clone, Copy)]
struct Issued {
    hwnd: u32,
    pid: u32,
    started: Option<u64>,
    class: Option<u64>,
}

impl Issued {
    fn binding(self) -> Binding {
        Binding {
            window_pid: Some(self.pid),
            started: self.started,
            class: self.class,
        }
    }
}

/// A screenshot this session took, for clicks given in its pixels.
#[derive(Debug, Clone, Copy)]
struct ShotMeta {
    source: Rect,
    scale_x: f64,
    scale_y: f64,
    width: u32,
    height: u32,
    /// The window it captured, held to its identity and place.
    window: Option<(u32, u64, Binding)>,
    /// The native monitor actually captured, independent of its current
    /// position in enumeration or whether it is still primary.
    monitor: Option<capture::MonitorSnapshot>,
}

/// How many screenshots stay addressable by handle.
const SHOTS_KEPT: usize = 8;

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

/// The handles this session has issued and what each is bound to.
///
/// A window handle names one window: the native id, the process that owned
/// it and that process's start time, and the window's class, as they were
/// when the handle was issued. The OS recycles native ids and pids; a
/// window seen later under the same native id with another identity gets a
/// handle of its own, and the old handle answers as gone. A pid is bound to
/// the start time of the process it named when first handed out, and is
/// never re-bound either.
#[derive(Debug, Default)]
pub struct Registry {
    windows: HashMap<u64, Issued>,
    /// The current handle for a native id.
    by_hwnd: HashMap<u32, u64>,
    next_window: u64,
    /// Every pid this session handed out (listed or launched), with its
    /// process's start time. A pid whose start time the OS cannot report is
    /// never recorded, so it is never accepted as a target.
    started: HashMap<u32, u64>,
    /// Listed pids whose start time could not be read (a protected process
    /// on Windows): refused as unidentifiable rather than as never listed,
    /// and never bound later, or a caller holding the pid from that listing
    /// would reach a successor.
    unidentified: HashSet<u32>,
    /// Window handles whose window this session saw gone (closed, or its
    /// native id taken by another process or class): never handed out
    /// again, so a later window that happens to match every recorded field
    /// under the same native id still gets a handle of its own.
    closed: RefCell<HashSet<u64>>,
    shots: VecDeque<(u64, ShotMeta)>,
    next_shot: u64,
}

impl Registry {
    fn handle(n: u64) -> String {
        format!("w{n}")
    }

    /// How an error names native window `hwnd` of `pid`.
    fn name(&self, hwnd: u32, pid: u32) -> WindowRef {
        let window = self
            .by_hwnd
            .get(&hwnd)
            .filter(|&&n| {
                !self.closed.borrow().contains(&n)
                    && self.windows.get(&n).is_some_and(|w| w.pid == pid)
            })
            .map(|&n| Self::handle(n));
        WindowRef {
            window,
            pid,
            title: os::window_title(hwnd),
        }
    }

    /// Issues, or finds, the handle for `w`, and whether the session accepts
    /// it as a target. `None` when the window closed or changed owner while
    /// it was listed: gone, not a window to hand out.
    ///
    /// The start time is read after the window list was taken, so it is kept
    /// only if the window still belongs to that pid afterwards: a window
    /// cannot outlive its process, so its owner was alive, under that pid,
    /// when the start time was read.
    fn adopt(&mut self, w: &NativeWindow) -> Option<(u64, Option<Untargetable>)> {
        let started = os::process_started(w.pid);
        let class = os::window_class(w.id);
        // A window with no class any more is being destroyed.
        if cfg!(target_os = "windows") && (os::window_pid(w.id) != Some(w.pid) || class.is_none()) {
            return None;
        }
        let reason = self.observe_process(w.pid, started);
        let n = self.register_window(Issued {
            hwnd: w.id,
            pid: w.pid,
            started,
            class,
        });
        Some((n, reason))
    }

    /// Targetability follows session history, not only this lookup: a pid
    /// handed out without an identity must never silently bind later.
    fn observe_process(&mut self, pid: u32, started: Option<u64>) -> Option<Untargetable> {
        if started.is_none() {
            self.unidentified.insert(pid);
        }
        if self.unidentified.contains(&pid) {
            return Some(Untargetable::UnidentifiedProcess);
        }
        if let Some(started) = started
            && *self.started.entry(pid).or_insert(started) != started
        {
            return Some(Untargetable::ReusedProcess);
        }
        None
    }

    /// Records an observed window identity. A displaced handle stays gone
    /// even if the OS later reuses the native id with its original fields.
    fn register_window(&mut self, observed: Issued) -> u64 {
        // The same window as before keeps its handle; a native id the OS
        // reused for another window or process gets a new one, and so does
        // one whose earlier window this session saw close.
        let same = self.by_hwnd.get(&observed.hwnd).copied().filter(|n| {
            !self.closed.borrow().contains(n)
                && self.windows.get(n).is_some_and(|issued| {
                    issued.pid == observed.pid
                        && issued.started == observed.started
                        && (issued.class.is_none()
                            || observed.class.is_none()
                            || issued.class == observed.class)
                })
        });
        same.unwrap_or_else(|| {
            if let Some(displaced) = self.by_hwnd.get(&observed.hwnd).copied() {
                self.close(displaced);
            }
            self.next_window += 1;
            let n = self.next_window;
            self.windows.insert(n, observed);
            self.by_hwnd.insert(observed.hwnd, n);
            n
        })
    }

    /// A selected window keeps the identity recorded when it was adopted.
    /// Never weaken its class fingerprint with another native lookup. The
    /// caller's process binding remains authoritative if selection raced
    /// with process replacement.
    fn selected_binding(&self, handle: u64, target: Binding) -> Binding {
        let issued = self
            .windows
            .get(&handle)
            .expect("BUG: a selected window was adopted before its binding is requested");
        Binding {
            started: target.started.or(issued.started),
            ..issued.binding()
        }
    }

    /// Marks every issued window whose native window is gone, or now
    /// another process's: seen once, its handle is never reused, whatever
    /// the OS later puts under the same native id.
    fn sweep(&mut self) -> ToolResult<()> {
        #[cfg(target_os = "macos")]
        {
            self.sweep_owners(os::macos_window_owners())
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Only where the OS answers who owns a native id directly:
            // elsewhere no answer is not "gone".
            if !cfg!(target_os = "windows") {
                return Ok(());
            }
            let owners = self
                .by_hwnd
                .keys()
                .filter_map(|&hwnd| os::window_pid(hwnd).map(|pid| (hwnd, pid)))
                .collect();
            self.sweep_owners(Ok(owners))
        }
    }

    /// A complete native owner snapshot includes windows hidden or on other
    /// Spaces; the capture list does not. A failed snapshot proves nothing
    /// about disappearance and must leave the handles intact.
    fn sweep_owners(&mut self, owners: ToolResult<HashMap<u32, u32>>) -> ToolResult<()> {
        let owners = owners?;
        let gone: Vec<_> = self
            .by_hwnd
            .iter()
            .filter_map(|(&hwnd, &handle)| {
                self.windows
                    .get(&handle)
                    .filter(|issued| owners.get(&hwnd) != Some(&issued.pid))
                    .map(|_| handle)
            })
            .collect();
        for handle in gone {
            self.close(handle);
        }
        Ok(())
    }

    /// Retires window handle `n` for good.
    fn close(&mut self, n: u64) {
        if let Some(issued) = self.windows.get(&n)
            && self.by_hwnd.get(&issued.hwnd) == Some(&n)
        {
            self.by_hwnd.remove(&issued.hwnd);
        }
        self.closed.borrow_mut().insert(n);
    }

    /// `w` as the tools report it, or `None` for one that closed meanwhile.
    fn window(&mut self, w: &NativeWindow) -> Option<Window> {
        let (n, reason) = self.adopt(w)?;
        Some(Window {
            id: Self::handle(n),
            pid: w.pid,
            app_name: w.app_name.clone(),
            title: w.title.clone(),
            rect: w.rect,
            is_minimized: w.is_minimized,
            is_focused: w.is_focused,
            targetable: reason.is_none(),
            untargetable_reason: reason,
        })
    }

    /// Binds `pid` to the process `launch` just started under it. The one
    /// re-binding there is: this session created that process and hands the
    /// pid out now, so it is the sighting the caller holds. `started` is
    /// read at the spawn, while the launcher still holds the child, so it
    /// cannot belong to a later process reusing the pid. A pid this session
    /// already handed out for another process is not re-bound. Whether the
    /// pid is bound comes back.
    fn bind_launched(&mut self, pid: u32, started: Option<u64>) -> bool {
        let Some(started) = started else {
            return false;
        };
        if let Some(&then) = self.started.get(&pid) {
            return then == started;
        }
        if self.unidentified.contains(&pid) {
            return false;
        }
        self.started.insert(pid, started);
        true
    }

    /// What a target is bound to. A handle never issued is unknown; one
    /// issued for a window that is gone, or a pid whose process has exited,
    /// is gone; a target the OS cannot identify is refused.
    fn bound(&mut self, target: TargetArg) -> ToolResult<(Target, Binding)> {
        match target {
            TargetArg::Window(n) => {
                let issued = *self
                    .windows
                    .get(&n)
                    .ok_or_else(|| ToolError::UnknownHandle {
                        handle: Self::handle(n),
                        kind: HandleKind::Window,
                    })?;
                if self.closed.borrow().contains(&n) {
                    return Err(ToolError::Gone {
                        handle: Self::handle(n),
                        kind: HandleKind::Window,
                        why: "it has closed".into(),
                    });
                }
                // On Windows every process has a start time unless it could
                // not be read: then nothing tells it from a successor.
                if issued.started.is_none() && cfg!(target_os = "windows") {
                    return Err(ToolError::NotSupported(format!(
                        "window w{n} belongs to process {}, which cannot be identified (its start time cannot be read), so it is not a safety target",
                        issued.pid
                    )));
                }
                let target = Target::Window(issued.hwnd, n);
                if let Err(e) = self.revalidate(target, issued.binding()) {
                    if matches!(e, ToolError::Gone { .. }) {
                        self.close(n);
                    }
                    return Err(e);
                }
                Ok((target, issued.binding()))
            }
            TargetArg::Pid(pid) => {
                let now = os::process_started(pid);
                let Some(&then) = self.started.get(&pid) else {
                    if self.unidentified.contains(&pid) {
                        return Err(ToolError::NotSupported(format!(
                            "process {pid} cannot be identified (its start time cannot be read), so it is not a safety target"
                        )));
                    }
                    if now.is_none() && !cfg!(target_os = "windows") {
                        // The OS has no start times: the reason to report.
                        same_process(pid, None, None)?;
                    }
                    return Err(ToolError::UnknownHandle {
                        handle: pid.to_string(),
                        kind: HandleKind::Process,
                    });
                };
                same_process(pid, Some(then), now)?;
                Ok((
                    Target::Pid(pid),
                    Binding {
                        window_pid: None,
                        started: Some(then),
                        class: None,
                    },
                ))
            }
        }
    }

    /// Refuses a target that no longer names what it was issued for: a
    /// window that closed or changed owner or class, or a process (the
    /// pid's, or the window owner's) whose start time changed — the OS can
    /// recycle both the native id and the pid of an application that exited.
    fn revalidate(&self, target: Target, bound: Binding) -> ToolResult<()> {
        self.observe(target, || Self::current_identity(target, bound))
    }

    /// Remember disappearance from every path, including a guard in the
    /// middle of an action. Otherwise a later matching native id can revive
    /// a handle that the session already reported gone.
    fn observe<T>(
        &self,
        target: Target,
        operation: impl FnOnce() -> ToolResult<T>,
    ) -> ToolResult<T> {
        if let Target::Window(_, n) = target
            && self.closed.borrow().contains(&n)
        {
            return Err(no_window(target));
        }
        let result = operation();
        if matches!(&result, Err(ToolError::Gone { .. }))
            && let Target::Window(_, n) = target
        {
            self.closed.borrow_mut().insert(n);
        }
        result
    }

    fn current_identity(target: Target, bound: Binding) -> ToolResult<()> {
        let pid = match target {
            Target::Pid(pid) => Some(pid),
            Target::Window(hwnd, n) => {
                let gone = |why: &str| ToolError::Gone {
                    handle: Self::handle(n),
                    kind: HandleKind::Window,
                    why: why.into(),
                };
                if let (Some(then), Some(now)) = (bound.class, os::window_class(hwnd))
                    && now != then
                {
                    return Err(gone(
                        "the OS reused its native id for another window (its class changed)",
                    ));
                }
                if let Some(pid) = bound.window_pid {
                    match Self::owner_now(hwnd)? {
                        None => return Err(gone("it has closed")),
                        Some(now) if now != pid => {
                            return Err(gone(
                                "the OS reused its native id for another process's window",
                            ));
                        }
                        Some(_) => {}
                    }
                }
                bound.window_pid
            }
        };
        if let (Some(pid), Some(then)) = (pid, bound.started) {
            same_process(pid, Some(then), os::process_started(pid)).map_err(|error| {
                match (target, error) {
                    (Target::Window(_, n), ToolError::Gone { why, .. }) => ToolError::Gone {
                        handle: Self::handle(n),
                        kind: HandleKind::Window,
                        why,
                    },
                    (_, error) => error,
                }
            })?;
        }
        Ok(())
    }

    /// The process that owns native window `hwnd` now: from the OS where it
    /// answers directly, including hidden windows on macOS. A failed query
    /// is not evidence of a closed window.
    fn owner_now(hwnd: u32) -> ToolResult<Option<u32>> {
        #[cfg(target_os = "macos")]
        {
            Ok(os::macos_window_owners()?.get(&hwnd).copied())
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(pid) = os::window_pid(hwnd) {
                return Ok(Some(pid));
            }
            Ok(capture::windows()?
                .into_iter()
                .find(|w| w.id == hwnd)
                .map(|w| w.pid))
        }
    }

    fn remember_shot(&mut self, meta: ShotMeta) -> String {
        self.next_shot += 1;
        self.shots.push_back((self.next_shot, meta));
        while self.shots.len() > SHOTS_KEPT {
            self.shots.pop_front();
        }
        format!("s{}", self.next_shot)
    }

    fn shot(&self, n: u64) -> ToolResult<ShotMeta> {
        if let Some(&(_, meta)) = self.shots.iter().find(|(id, _)| *id == n) {
            return Ok(meta);
        }
        Err(if n <= self.next_shot && n > 0 {
            ToolError::Gone {
                handle: format!("s{n}"),
                kind: HandleKind::Screenshot,
                why: format!("only the last {SHOTS_KEPT} screenshots stay addressable"),
            }
        } else {
            ToolError::UnknownHandle {
                handle: format!("s{n}"),
                kind: HandleKind::Screenshot,
            }
        })
    }
}

/// What an element action left behind.
#[derive(Debug)]
pub struct ActOutcome {
    /// The element afterwards, when it could be read back.
    pub element: Option<Node>,
    /// Why it could not be, when it could not: the action itself ran.
    pub readback: Option<ToolError>,
}

/// Where `activate_window` left things.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Activated {
    /// The window it raised.
    pub window: Window,
    /// Whether that window is the foreground window now.
    pub became_foreground: bool,
    /// The foreground window now, when there is one.
    pub foreground: Option<WindowRef>,
}

/// Session state on the worker thread.
pub struct Desktop {
    a11y: Box<dyn AccessibilityBackend>,
    input: Result<Input, String>,
    registry: Registry,
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
            registry: Registry::default(),
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

    /// Binds a pid `launch` just started; see [`Registry::bind_launched`].
    pub fn bind_launched(&mut self, pid: u32, started: Option<u64>) -> bool {
        self.registry.bind_launched(pid, started)
    }

    /// The windows of the process this session bound `pid` to, or `None`
    /// once that process is gone: the pid may already name another process,
    /// whose windows are neither returned nor bound. A pid never bound is
    /// unknown.
    pub fn windows_of(&mut self, pid: u32) -> ToolResult<Option<Vec<Window>>> {
        let Some(&started) = self.registry.started.get(&pid) else {
            return Err(ToolError::UnknownHandle {
                handle: pid.to_string(),
                kind: HandleKind::Process,
            });
        };
        let same = || os::process_started(pid) == Some(started);
        if !same() {
            return Ok(None);
        }
        let windows: Vec<NativeWindow> = capture::windows()?
            .into_iter()
            .filter(|w| w.pid == pid)
            .collect();
        if !same() {
            return Ok(None);
        }
        Ok(Some(
            windows
                .iter()
                .filter_map(|w| self.registry.window(w))
                .collect(),
        ))
    }

    /// The window list, optionally filtered. Every window it returns gets
    /// its handle, and every pid is bound the first time it is seen.
    pub fn list_windows(
        &mut self,
        title_contains: Option<&str>,
        pid: Option<u32>,
    ) -> ToolResult<Vec<Window>> {
        self.registry.sweep()?;
        let needle = title_contains.map(a11y::fold);
        let windows: Vec<NativeWindow> = capture::windows()?
            .into_iter()
            .filter(|w| pid.is_none_or(|p| w.pid == p))
            .filter(|w| {
                needle
                    .as_deref()
                    .is_none_or(|n| a11y::fold(&w.title).contains(n))
            })
            .collect();
        Ok(windows
            .iter()
            .filter_map(|w| self.registry.window(w))
            .collect())
    }

    /// The native windows a target names: one window, or a process's
    /// windows front to back.
    fn resolve(target: Target) -> ToolResult<Vec<NativeWindow>> {
        let all = capture::windows()?;
        let found: Vec<_> = match target {
            // The capture list leaves popups out; the OS still knows them.
            Target::Window(hwnd, _) => all
                .iter()
                .find(|w| w.id == hwnd)
                .cloned()
                .or_else(|| os_window(hwnd))
                .into_iter()
                .collect(),
            Target::Pid(pid) => all.into_iter().filter(|w| w.pid == pid).collect(),
        };
        if found.is_empty() {
            if let Target::Window(hwnd, n) = target
                && Registry::owner_now(hwnd)?.is_some()
            {
                return Err(ToolError::NotFound(format!(
                    "window w{n} still exists but is not available in the capture list"
                )));
            }
            return Err(no_window(target));
        }
        Ok(found)
    }

    /// Captures a window, a process's frontmost window, or a monitor, and
    /// remembers where the pixels came from under the handle returned.
    ///
    /// A window or pid is held to the identity it was issued with, before the
    /// capture and again after it: pixels of an application that took over a
    /// recycled id are refused, not returned.
    pub fn screenshot(
        &mut self,
        target: ScreenshotTarget,
        max_side: Option<u32>,
    ) -> ToolResult<(Shot, String)> {
        capture::available()?;
        let (direct, held) = match target {
            ScreenshotTarget::Primary => (ShotTarget::Primary, None),
            ScreenshotTarget::Monitor(m) => (ShotTarget::Monitor(m), None),
            ScreenshotTarget::Window(n) => {
                let (target, bound) = self.registry.bound(TargetArg::Window(n))?;
                let Target::Window(hwnd, _) = target else {
                    unreachable!("a window handle resolves to a window");
                };
                (ShotTarget::Window(hwnd), Some((target, bound)))
            }
            ScreenshotTarget::Pid(pid) => {
                // Held to its identity like any other target: without one
                // (macOS) a reused pid would capture another application's
                // window, so the pid is refused and a window handle is the way.
                let (_, bound) = self.registry.bound(TargetArg::Pid(pid))?;
                let windows = read_while_current(
                    || self.registry.revalidate(Target::Pid(pid), bound),
                    || Self::resolve(Target::Pid(pid)),
                )?;
                let pick = windows
                    .iter()
                    .find(|w| w.is_focused)
                    .or_else(|| windows.iter().find(|w| !w.is_minimized))
                    .unwrap_or(&windows[0]);
                let Some((n, _)) = self.registry.adopt(pick) else {
                    self.registry.revalidate(Target::Pid(pid), bound)?;
                    return Err(ToolError::Busy(format!(
                        "the window of process {pid} closed while it was picked"
                    )));
                };
                let bound = self.registry.selected_binding(n, bound);
                (
                    ShotTarget::Window(pick.id),
                    Some((Target::Window(pick.id, n), bound)),
                )
            }
        };
        let passing = |e: ToolError| match target {
            // For a pid, the window it picked closing is passing: the same
            // call picks another of its windows.
            ScreenshotTarget::Pid(pid) => selected_window_error(Target::Pid(pid), e),
            _ => e,
        };
        let recheck = || {
            // A chosen window closing is transient for a pid; the process
            // itself exiting is final and must retain its process handle.
            if let ScreenshotTarget::Pid(pid) = target
                && let Some((_, bound)) = held
            {
                self.registry.revalidate(Target::Pid(pid), bound)?;
            }
            held.map_or(Ok(()), |(t, b)| self.registry.revalidate(t, b))
                .map_err(passing)
        };
        let shot = read_while_current(recheck, || {
            capture::screenshot(direct, max_side).map_err(passing)
        })?;
        let id = self.registry.remember_shot(ShotMeta {
            source: shot.source,
            scale_x: shot.scale_x,
            scale_y: shot.scale_y,
            width: shot.width,
            height: shot.height,
            window: held.and_then(|(t, b)| match t {
                Target::Window(hwnd, n) => Some((hwnd, n, b)),
                Target::Pid(_) => None,
            }),
            monitor: shot.monitor,
        });
        Ok((shot, id))
    }

    /// The native windows a scope's target names, held to their identity.
    fn scoped_windows(&mut self, target: TargetArg) -> ToolResult<(Target, Binding, Vec<u32>)> {
        let (target, bound) = self.registry.bound(target)?;
        let ids: Vec<u32> = read_while_current(
            || self.registry.revalidate(target, bound),
            || {
                self.registry.observe(target, || {
                    Ok(match target {
                        Target::Pid(pid) => match os::process_windows(pid)? {
                            Some(ids) if !ids.is_empty() => ids,
                            Some(_) => return Err(no_window(target)),
                            None => Self::resolve(target)?.iter().map(|w| w.id).collect(),
                        },
                        Target::Window(_, _) => {
                            Self::resolve(target)?.iter().map(|w| w.id).collect()
                        }
                    })
                })
            },
        )?;
        Ok((target, bound, ids))
    }

    /// The element trees a scope names: a window's, every window of a
    /// process (its popups — menus, drop-downs — are windows of their own
    /// and are read too where the OS lists them), or one element's subtree.
    /// At most `max_nodes` elements are fetched.
    ///
    /// The target is held to the identity it was issued with, as for input:
    /// a recycled window id or pid would otherwise hand out element handles
    /// in an unrelated application, which a later `set_value` or `invoke`
    /// (which take no target) would then act on.
    pub fn tree(
        &mut self,
        scope: &Scope,
        max_depth: usize,
        max_nodes: usize,
        deadline: Instant,
    ) -> ToolResult<Read> {
        self.a11y.available()?;
        let target = match scope {
            Scope::Element(root) => return self.a11y.subtree(root, max_depth, max_nodes, deadline),
            Scope::Target(target) => *target,
        };
        let (target, bound, ids) = self.scoped_windows(target)?;
        let mut read = read_while_current(
            || self.registry.revalidate(target, bound),
            || self.a11y.tree(&ids, max_depth, max_nodes, deadline),
        )?;
        let owner = match target {
            Target::Pid(pid) => Some(pid),
            Target::Window(_, _) => bound.window_pid,
        };
        if let Some(pid) = owner
            && let Some((id, now)) = ids.iter().find_map(|&id| {
                os::window_pid(id)
                    .filter(|&now| now != pid)
                    .map(|now| (id, now))
            })
        {
            let changed =
                format!("a window of the target changed owner to process {now} while it was read");
            return self.registry.observe(target, || {
                Err(match target {
                    Target::Pid(_) => ToolError::Busy(changed),
                    Target::Window(_, n) => ToolError::Gone {
                        handle: Registry::handle(n),
                        kind: HandleKind::Window,
                        why: format!("the OS reused native window {id}: {changed}"),
                    },
                })
            });
        }
        // Each root is named after its window, so a popup or dialog read
        // under a pid can be targeted by handle afterwards.
        // The capture list leaves popups out (menus, drop-downs, tooltips),
        // which the OS enumeration read: those are described from the OS.
        let all = capture::windows().unwrap_or_default();
        for root in &mut read.roots {
            let Some(hwnd) = root.native_window else {
                continue;
            };
            let native = all
                .iter()
                .find(|w| w.id == hwnd)
                .cloned()
                .or_else(|| os_window(hwnd));
            if let Some(w) = native
                && let Some((n, _)) = self.registry.adopt(&w)
            {
                root.window = Some(Registry::handle(n));
            }
        }
        Ok(read)
    }

    /// Elements matching `query` anywhere in a scope, and the read they came
    /// from.
    pub fn find(
        &mut self,
        scope: &Scope,
        query: &Query,
        deadline: Instant,
    ) -> ToolResult<(Vec<Node>, Read)> {
        if let Some(element) = &query.element {
            self.a11y.validate_handle(element)?;
        }
        // Bounded like `accessibility_tree`: an unbounded walk of a deeply
        // nested tree (a browser's, a document's) recurses until the worker's
        // stack runs out.
        let read = self.tree(
            scope,
            crate::params::MAX_DEPTH as usize,
            crate::params::MAX_NODES as usize,
            deadline,
        )?;
        Ok((a11y::search(&read.roots, query), read))
    }

    /// Performs an element action. An action that ran but whose element
    /// could not be read back afterwards is a success with no element and
    /// the reason, not a failure: repeating it would repeat the action.
    pub fn act(&mut self, element: &str, action: &Action) -> ToolResult<ActOutcome> {
        match self.a11y.act(element, action) {
            Ok(node) => Ok(ActOutcome {
                element: Some(node),
                readback: None,
            }),
            Err(ToolError::Interrupted {
                cause,
                effect: Effect::Ran,
                ..
            }) => Ok(ActOutcome {
                element: None,
                readback: Some(*cause),
            }),
            Err(e) => Err(e),
        }
    }

    /// The foreground window, read straight from the OS where it can be, so
    /// the check before every input event stays cheap.
    fn foreground(registry: &Registry) -> ToolResult<Option<Foreground>> {
        let named = |id: u32, pid: u32, title: Option<String>, rect: Option<Rect>| Foreground {
            id,
            pid,
            handle: registry.name(id, pid).window,
            title,
            rect,
        };
        if let Some((id, pid)) = os::foreground()
            && let Some(rect) = os::window_rect(id)
        {
            return Ok(Some(named(id, pid, os::window_title(id), Some(rect))));
        }
        let windows = capture::windows()?;
        Ok(match os::foreground() {
            Some((id, pid)) => {
                let listed = windows.iter().find(|w| w.id == id);
                Some(named(
                    id,
                    pid,
                    listed.map(|w| w.title.clone()),
                    listed.map(|w| w.rect),
                ))
            }
            None => windows
                .iter()
                .find(|w| w.is_focused)
                .map(|w| named(w.id, w.pid, Some(w.title.clone()), Some(w.rect))),
        })
    }

    fn check(
        registry: &Registry,
        target: Target,
        bound: Binding,
        points: &[(i32, i32)],
    ) -> ToolResult<()> {
        // Shutting down: the action in progress stops at its next event and
        // releases what it holds, instead of the server exiting under it.
        if STOPPING.load(Ordering::SeqCst) {
            return Err(ToolError::ShuttingDown);
        }
        registry.revalidate(target, bound)?;
        let fg = Self::foreground(registry)?;
        let name = |hwnd, pid| registry.name(hwnd, pid);
        if points.is_empty() {
            // Keys and text: they go to the focused window, which must be
            // the target's process too; the focus is read on the very
            // window just checked.
            verify(target, fg.as_ref(), &[], &name)?;
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
        verify(target, fg.as_ref(), &points, &name)
    }

    fn check_element(
        registry: &Registry,
        window: u32,
        pid: u32,
        (x, y): (i32, i32),
    ) -> ToolResult<()> {
        let fg = Self::foreground(registry)?;
        let name = |hwnd, pid| registry.name(hwnd, pid);
        verify_element(window, pid, fg.as_ref(), (x, y), os::window_at(x, y), &name)
    }

    /// The screen point a location names, and for an element its own window
    /// and process, which a press on it is checked against.
    fn locate(&mut self, at: &Location) -> ToolResult<(i32, i32, Option<ElementWindow>)> {
        match at {
            Location::Point(x, y) => Ok((*x, *y, None)),
            Location::InScreenshot { shot, x, y } => {
                let meta = self.registry.shot(*shot)?;
                if *x < 0 || *y < 0 || *x as u32 >= meta.width || *y as u32 >= meta.height {
                    return Err(ToolError::InvalidArgument(format!(
                        "({x}, {y}) is outside screenshot s{shot}, which is {}x{} pixels",
                        meta.width, meta.height
                    )));
                }
                // The pixels describe the screen only while what they show
                // is still where it was.
                // Unreadable counts as changed: stale pixels must not aim.
                if let Some((hwnd, n, bound)) = meta.window {
                    self.registry.revalidate(Target::Window(hwnd, n), bound)?;
                    let now = os::window_rect(hwnd).or_else(|| {
                        capture::windows()
                            .ok()?
                            .into_iter()
                            .find(|w| w.id == hwnd)
                            .map(|w| w.rect)
                    });
                    if now != Some(meta.source) {
                        return Err(ToolError::Busy(format!(
                            "window w{n} has moved or resized since screenshot s{shot} (or its bounds cannot be read); take another"
                        )));
                    }
                }
                if let Some(monitor) = meta.monitor {
                    capture::verify_monitor(monitor, capture::monitor_snapshot(monitor.id))?;
                }
                // Down to the screen unit the pixel lies in, and never past
                // the captured rect's far edge (a rounded HiDPI pixel would).
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a pixel of a bounded image divided by a positive scale"
                )]
                let map = |px: i32, origin: i32, span: u32, scale: f64| {
                    let unit = (f64::from(px) / scale.max(f64::EPSILON)).floor() as i64;
                    let last = i64::from(span).saturating_sub(1).max(0);
                    let at = i64::from(origin) + unit.clamp(0, last);
                    i32::try_from(at).unwrap_or(origin)
                };
                Ok((
                    map(*x, meta.source.x, meta.source.width, meta.scale_x),
                    map(*y, meta.source.y, meta.source.height, meta.scale_y),
                    None,
                ))
            }
            Location::Element(handle) => {
                let p = self.a11y.click_point(handle)?;
                // The element's own top-level window: a sibling window of the
                // same process in front would take a click checked against the
                // process alone, so an unknown window is refused, not relaxed.
                let window = p.window.ok_or_else(|| {
                    ToolError::NotSupported(format!(
                        "cannot tell which window element `{handle}` belongs to, so it is not clicked; use invoke"
                    ))
                })?;
                let Some(pid) = os::window_pid(window) else {
                    self.a11y.invalidate_handle(handle);
                    return Err(ToolError::gone_element(handle, "its window has closed"));
                };
                Ok((p.x, p.y, Some((handle.clone(), window, pid))))
            }
        }
    }

    /// Refuses unless an element location still names what is at its point:
    /// the element itself (or a descendant) is what UI Automation hit-tests
    /// there, its own top-level window is under the point and its process is
    /// in front. Nothing for a point location.
    fn element_still_there(
        a11y: &mut dyn AccessibilityBackend,
        registry: &Registry,
        element: Option<&ElementWindow>,
        (x, y): (i32, i32),
    ) -> ToolResult<()> {
        Self::element_hit(a11y, element, (x, y))?;
        Self::element_window(registry, element, (x, y))
    }

    /// The UI Automation half of [`Self::element_still_there`]: the element
    /// (or a descendant) is what is hit-tested at the point. A cross-process
    /// call, so it runs before the fast checks.
    fn element_hit(
        a11y: &mut dyn AccessibilityBackend,
        element: Option<&ElementWindow>,
        (x, y): (i32, i32),
    ) -> ToolResult<()> {
        let Some((handle, _, _)) = element else {
            return Ok(());
        };
        if a11y.hits(handle, x, y)? {
            return Ok(());
        }
        Err(ToolError::OutsideTarget {
            x,
            y,
            reason: format!(
                "element `{handle}` is no longer what is under it (something inside its window covers it); use invoke, or read the tree again"
            ),
            covered_by: None,
        })
    }

    /// The OS half: the element's own window is under the point and its
    /// process in front.
    fn element_window(
        registry: &Registry,
        element: Option<&ElementWindow>,
        point: (i32, i32),
    ) -> ToolResult<()> {
        match element {
            Some((_, window, pid)) => Self::check_element(registry, *window, *pid, point),
            None => Ok(()),
        }
    }

    /// Clicks an element's clickable point or a screen point.
    pub fn click(
        &mut self,
        at: &Location,
        button: MouseButton,
        double: bool,
        target: TargetArg,
    ) -> ToolResult<(i32, i32)> {
        self.input()?;
        let (target, bound) = self.registry.bound(target)?;
        let (x, y, element_window) = self.locate(at)?;
        let Self {
            a11y,
            input,
            registry,
        } = self;
        let input = input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))?;
        let mut guard = |_: Option<(i32, i32)>| {
            // An element click always lands on the element. The slow check
            // (a UI Automation hit-test, a cross-process call) runs first and
            // the fast OS checks last, right before the event, so what they
            // saw is as fresh as it can be.
            Self::element_still_there(a11y.as_mut(), registry, element_window.as_ref(), (x, y))?;
            Self::check(registry, target, bound, &[(x, y)])
        };
        guard(None)?;
        input.click(x, y, button, double, &mut guard)?;
        Ok((x, y))
    }

    /// Moves the pointer; where it was asked to go, and where it is.
    pub fn move_mouse(&mut self, at: &Location) -> ToolResult<(Point, Option<Point>)> {
        // Shutting down: no pointer moves once the session has ended.
        if STOPPING.load(Ordering::SeqCst) {
            return Err(ToolError::ShuttingDown);
        }
        self.input()?;
        let (x, y, _) = self.locate(at)?;
        let input = self.input()?;
        // A move with a button still held from a failed release is a drag.
        input.ready()?;
        // Again right before the move: resolving an element's point can take
        // a provider's whole call timeout, and shutdown may have begun.
        if STOPPING.load(Ordering::SeqCst) {
            return Err(ToolError::ShuttingDown);
        }
        input.move_to(x, y)?;
        std::thread::sleep(Duration::from_millis(10));
        Ok(((x, y), input.position()))
    }

    /// Drags with the left button. The start, the end and every point the
    /// drag passes through are checked before the press, and each step
    /// checks the point it is about to reach again before moving there.
    pub fn drag(
        &mut self,
        from: &Location,
        to: &Location,
        duration: Duration,
        target: TargetArg,
    ) -> ToolResult<((i32, i32), (i32, i32))> {
        self.input()?;
        let (target, bound) = self.registry.bound(target)?;
        let (fx, fy, from_element) = self.locate(from)?;
        let (tx, ty, to_element) = self.locate(to)?;
        let (from, to) = ((fx, fy), (tx, ty));
        let mut path = vec![from];
        path.extend(crate::input::drag_path(from, to, duration));
        let Self {
            a11y,
            input,
            registry,
        } = self;
        Self::check(registry, target, bound, &path)?;
        // Before the press, the ends given as elements are still those
        // elements: covered by the target at their old points, the drag
        // would act on the target instead.
        let mut guard = |at: Option<(i32, i32)>| {
            if let Some(point) = at {
                return Self::check(registry, target, bound, &[point]);
            }
            // The slow hit-tests first, the fast window checks after both,
            // so what they saw is as fresh as it can be.
            Self::element_hit(a11y.as_mut(), from_element.as_ref(), from)?;
            Self::element_hit(a11y.as_mut(), to_element.as_ref(), to)?;
            Self::element_window(registry, from_element.as_ref(), from)?;
            Self::element_window(registry, to_element.as_ref(), to)?;
            Self::check(registry, target, bound, &path)
        };
        input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))?
            .drag(from, to, duration, &mut guard)?;
        Ok((from, to))
    }

    /// Scrolls at a point.
    pub fn scroll(
        &mut self,
        at: &Location,
        dx: i32,
        dy: i32,
        target: TargetArg,
    ) -> ToolResult<(i32, i32)> {
        self.input()?;
        let (target, bound) = self.registry.bound(target)?;
        let (x, y, element) = self.locate(at)?;
        let Self {
            a11y,
            input,
            registry,
        } = self;
        let mut guard = |_: Option<(i32, i32)>| {
            Self::element_still_there(a11y.as_mut(), registry, element.as_ref(), (x, y))?;
            Self::check(registry, target, bound, &[(x, y)])
        };
        guard(None)?;
        input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))?
            .scroll(x, y, dx, dy, &mut guard)?;
        Ok((x, y))
    }

    /// Types text into whatever has keyboard focus.
    pub fn type_text(&mut self, text: &str, target: TargetArg) -> ToolResult<usize> {
        self.input()?;
        let (target, bound) = self.registry.bound(target)?;
        let registry = &self.registry;
        Self::check(registry, target, bound, &[])?;
        let mut guard = |_: Option<(i32, i32)>| Self::check(registry, target, bound, &[]);
        self.input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))?
            .type_text(text, &mut guard)?;
        Ok(text.chars().count())
    }

    /// Presses a key combo.
    pub fn key(&mut self, combo: &KeyCombo, repeat: u32, target: TargetArg) -> ToolResult<()> {
        self.input()?;
        if let Some(handler) = combo.shell_hotkey(cfg!(target_os = "macos")) {
            return Err(ToolError::NotSupported(format!(
                "`{combo}` is handled by {handler}, not by the target window, so no safety target can hold for it; it is refused"
            )));
        }
        let (target, bound) = self.registry.bound(target)?;
        let registry = &self.registry;
        Self::check(registry, target, bound, &[])?;
        let mut guard = |_: Option<(i32, i32)>| Self::check(registry, target, bound, &[]);
        self.input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))?
            .key(combo, repeat, &mut guard)
    }

    /// Brings a window to the front and reports whether it got there. A
    /// window handle must be one this session issued, still naming the same
    /// window; activating never re-binds anything.
    pub fn activate(&mut self, target: TargetArg) -> ToolResult<Activated> {
        if !cfg!(target_os = "windows") {
            return Err(ToolError::NotSupported(format!(
                "activate_window is not supported on {} yet",
                std::env::consts::OS
            )));
        }
        let (target, bound) = self.registry.bound(target)?;
        let windows = read_while_current(
            || self.registry.revalidate(target, bound),
            || self.registry.observe(target, || Self::resolve(target)),
        )?;
        // A window that closed meanwhile is dropped; one whose own id cannot
        // be targeted still belongs to the target (a pid reaches it).
        let native = windows
            .iter()
            .find(|w| !w.is_minimized)
            .or_else(|| windows.first())
            .ok_or_else(|| no_window(target))?;
        let Some(window) = self.registry.window(native) else {
            self.registry.revalidate(target, bound)?;
            return Err(ToolError::Busy(
                "the window closed while it was picked".into(),
            ));
        };
        let n = self
            .registry
            .by_hwnd
            .get(&native.id)
            .copied()
            .expect("BUG: a window just adopted has a handle");
        // The chosen window itself, not only its process, is what gets
        // raised: held to its owner, the process's start time and its class
        // as resolved, right before each attempt.
        let chosen = self.registry.selected_binding(n, bound);
        let recheck = || {
            self.registry.revalidate(target, bound)?;
            self.registry
                .revalidate(Target::Window(native.id, n), chosen)
        };
        let (fg, refreshed) = activate_while_current(recheck, || {
            os::bring_to_front(native.id)?;
            let foreground = Self::foreground(&self.registry)?;
            if foreground.as_ref().map(|f| f.id) != Some(native.id) {
                recheck()?;
                if let Err(e) = self.a11y.focus_window(native.id) {
                    tracing::debug!("UIA focus fallback failed: {e}");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let mut refreshed = Self::resolve(Target::Window(native.id, n))?
                .into_iter()
                .next()
                .ok_or_else(|| no_window(Target::Window(native.id, n)))?;
            let fg = Self::foreground(&self.registry)?;
            refreshed.is_focused = fg.as_ref().map(|f| f.id) == Some(native.id);
            Ok((fg, refreshed))
        })
        .map_err(|error| selected_window_error(target, error))?;
        let became = fg.as_ref().map(|f| f.id) == Some(native.id);
        Ok(Activated {
            window: refreshed_window(window, refreshed),
            became_foreground: became,
            foreground: fg.as_ref().map(Foreground::as_ref),
        })
    }
}

/// A pid can select another window on a later call, so losing only its
/// chosen window is transient. Preserve process disappearance and any
/// action effects: transient selection does not justify replaying an
/// action that may already have run.
fn selected_window_error(target: Target, error: ToolError) -> ToolError {
    if !matches!(target, Target::Pid(_)) {
        return error;
    }
    match error {
        ToolError::Gone {
            kind: HandleKind::Window,
            why,
            ..
        }
        | ToolError::NotFound(why) => ToolError::Busy(why),
        ToolError::Interrupted {
            cause,
            effect,
            detail,
        } => ToolError::Interrupted {
            cause: Box::new(selected_window_error(target, *cause)),
            effect,
            detail,
        },
        other => other,
    }
}

/// Fresh observable fields after activation, retaining the session handle
/// and its targetability decision rather than adopting another identity.
fn refreshed_window(window: Window, refreshed: NativeWindow) -> Window {
    Window {
        pid: refreshed.pid,
        app_name: refreshed.app_name,
        title: refreshed.title,
        rect: refreshed.rect,
        is_minimized: refreshed.is_minimized,
        is_focused: refreshed.is_focused,
        ..window
    }
}

/// A window the capture list does not show (a popup), described from the
/// OS.
fn os_window(hwnd: u32) -> Option<NativeWindow> {
    Some(NativeWindow {
        id: hwnd,
        pid: os::window_pid(hwnd)?,
        app_name: String::new(),
        title: os::window_title(hwnd).unwrap_or_default(),
        rect: os::window_rect(hwnd)?,
        is_minimized: false,
        is_focused: false,
    })
}

fn no_window(target: Target) -> ToolError {
    match target {
        Target::Window(_, n) => ToolError::Gone {
            handle: Registry::handle(n),
            kind: HandleKind::Window,
            why: "it is not listed any more (closed)".into(),
        },
        Target::Pid(pid) => ToolError::NotFound(format!(
            "process {pid} has no top-level window; if the app hands off to another \
             process (Windows 11 Notepad does), find that one with list_windows"
        )),
    }
}

/// Reads are held to an identity on both success and failure. A backend's
/// `not_found` or generic capture failure is not the final diagnosis when
/// the issued target closed meanwhile. A still-live target keeps the
/// backend error (for example, a popup xcap does not capture).
fn read_while_current<T>(
    mut check: impl FnMut() -> ToolResult<()>,
    read: impl FnOnce() -> ToolResult<T>,
) -> ToolResult<T> {
    check()?;
    let result = read();
    check()?;
    result
}

/// Activation may restore or focus a window before either the backend or
/// the final identity check fails. Preserve the final handle diagnosis and
/// tell the caller that part of that action may already have happened.
fn activate_while_current<T>(
    check: impl FnMut() -> ToolResult<()>,
    activate: impl FnOnce() -> ToolResult<T>,
) -> ToolResult<T> {
    let began = std::cell::Cell::new(false);
    read_while_current(check, || {
        began.set(true);
        activate()
    })
    .map_err(|error| {
        if began.get() {
            error.after(
                Effect::MayHaveRun,
                "activation may have restored or focused the window before it failed",
            )
        } else {
            error
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_uses_the_adopted_class_and_keeps_the_original_process_binding() {
        let mut registry = Registry::default();
        let issued = Issued {
            hwnd: 7,
            pid: 100,
            started: Some(456),
            class: Some(789),
        };
        let handle = registry.register_window(issued);
        let selected = registry.selected_binding(
            handle,
            Binding {
                started: Some(123),
                ..Binding::default()
            },
        );
        assert_eq!(
            selected.class,
            Some(789),
            "selection must retain the recorded class without another native query"
        );
        assert_eq!(selected.window_pid, Some(100));
        assert_eq!(
            selected.started,
            Some(123),
            "adoption must not replace the caller's process identity"
        );
        assert_eq!(
            same_process(100, selected.started, Some(456))
                .expect_err("BUG: a successor process is refused")
                .code(),
            "gone"
        );
        let selected = registry.selected_binding(handle, Binding::default());
        assert_eq!(selected.class, Some(789));
        assert_eq!(selected.started, issued.started);
    }

    #[test]
    fn a_reused_readable_pid_is_not_advertised_as_targetable() {
        let mut registry = Registry::default();
        assert_eq!(registry.observe_process(100, Some(123)), None);
        assert_eq!(
            registry.observe_process(100, Some(456)),
            Some(Untargetable::ReusedProcess)
        );
        assert!(!registry.bind_launched(100, Some(456)));
        assert_eq!(registry.started.get(&100), Some(&123));
        assert_eq!(
            same_process(100, registry.started.get(&100).copied(), Some(456))
                .expect_err("BUG: the old pid binding remains final")
                .code(),
            "gone"
        );
        assert_eq!(
            registry.observe_process(100, Some(456)),
            Some(Untargetable::ReusedProcess)
        );
        let replacement = Issued {
            hwnd: 7,
            pid: 100,
            started: Some(456),
            class: Some(1),
        };
        let handle = registry.register_window(replacement);
        assert_eq!(registry.register_window(replacement), handle);
        assert!(
            registry
                .observe(Target::Window(7, handle), || Ok(()))
                .is_ok(),
            "the replacement window keeps its independent current handle"
        );
    }

    #[test]
    fn losing_a_selected_window_is_transient_only_for_a_pid_and_keeps_effects() {
        let window = Target::Window(7, 1);
        let plain = selected_window_error(Target::Pid(100), no_window(window));
        assert_eq!(plain.code(), "busy");
        assert_eq!(plain.retry(), crate::error::Retry::Soon);
        assert_eq!(
            selected_window_error(window, no_window(window)).code(),
            "gone"
        );
        let interrupted = selected_window_error(
            Target::Pid(100),
            no_window(window).after(Effect::MayHaveRun, "the restore was attempted"),
        );
        assert_eq!(interrupted.code(), "busy");
        assert_eq!(
            interrupted.payload()["error"]["effect"]["kind"],
            "may_have_run"
        );
        assert_eq!(
            interrupted.payload()["error"]["effect"]["detail"],
            "the restore was attempted"
        );
        assert_eq!(interrupted.retry(), crate::error::Retry::Never);
        let process = ToolError::Gone {
            handle: "100".into(),
            kind: HandleKind::Process,
            why: "exited".into(),
        };
        let error = selected_window_error(Target::Pid(100), process);
        assert_eq!(error.code(), "gone");
        assert_eq!(error.payload()["error"]["kind"], "process");
    }

    #[test]
    fn selection_distinguishes_an_exited_process_from_one_with_no_windows() {
        for exits_during_selection in [false, true] {
            let now = std::cell::Cell::new(Some(123));
            let result = read_while_current(
                || same_process(100, Some(123), now.get()),
                || {
                    if exits_during_selection {
                        now.set(None);
                    }
                    Err::<(), _>(no_window(Target::Pid(100)))
                },
            );
            let error = result.expect_err("BUG: selection found no window");
            if exits_during_selection {
                assert_eq!(error.code(), "gone");
                assert_eq!(error.payload()["error"]["kind"], "process");
            } else {
                assert_eq!(error.code(), "not_found");
                assert_eq!(error.retry(), crate::error::Retry::WhenAppears);
            }
        }
    }

    #[test]
    fn a_later_readable_identity_does_not_make_an_unidentified_pid_targetable() {
        let mut registry = Registry::default();
        assert_eq!(
            registry.observe_process(100, None),
            Some(Untargetable::UnidentifiedProcess)
        );
        assert_eq!(
            registry.observe_process(100, Some(123)),
            Some(Untargetable::UnidentifiedProcess)
        );
        assert!(!registry.bind_launched(100, Some(123)));
        assert_eq!(
            registry
                .bound(TargetArg::Pid(100))
                .expect_err("BUG: unidentifiable pid remains unbound")
                .code(),
            "not_supported"
        );
        assert_eq!(registry.observe_process(200, Some(456)), None);
        assert!(registry.bind_launched(200, Some(456)));
    }

    #[test]
    fn activation_refresh_keeps_the_handle_and_updates_observable_fields() {
        let before = Window {
            id: "w3".into(),
            pid: 100,
            app_name: "app".into(),
            title: "before".into(),
            rect: Rect {
                x: -32000,
                y: -32000,
                width: 160,
                height: 28,
            },
            is_minimized: true,
            is_focused: false,
            targetable: true,
            untargetable_reason: None,
        };
        let after = NativeWindow {
            id: 7,
            pid: 100,
            app_name: "app".into(),
            title: "after".into(),
            rect: Rect {
                x: 10,
                y: 20,
                width: 800,
                height: 600,
            },
            is_minimized: false,
            is_focused: true,
        };
        let updated = refreshed_window(before, after.clone());
        assert_eq!(updated.id, "w3");
        assert_eq!(updated.title, "after");
        assert_eq!(updated.rect, after.rect);
        assert!(!updated.is_minimized);
        assert!(updated.is_focused);
        assert!(updated.targetable);
    }

    #[test]
    fn complete_owner_snapshots_retire_only_confirmed_missing_windows() {
        let mut registry = Registry::default();
        let identity = Issued {
            hwnd: 7,
            pid: 100,
            started: None,
            class: None,
        };
        let handle = registry.register_window(identity);
        // The full native list still includes a hidden window even though
        // it is absent from the capture backend's on-screen-only list.
        registry
            .sweep_owners(Ok(HashMap::from([(7, 100)])))
            .expect("BUG: complete snapshot is accepted");
        assert_eq!(registry.register_window(identity), handle);
        let failed = registry.sweep_owners(Err(ToolError::platform(
            "listing window owners",
            "injected native query failure",
        )));
        assert!(failed.is_err());
        assert_eq!(registry.register_window(identity), handle);
        registry
            .sweep_owners(Ok(HashMap::new()))
            .expect("BUG: empty complete snapshot is accepted");
        let replacement = registry.register_window(identity);
        assert_ne!(replacement, handle);
        assert!(
            registry
                .observe(Target::Window(identity.hwnd, handle), || Ok(()))
                .is_err()
        );
        registry
            .sweep_owners(Ok(HashMap::from([(7, 200)])))
            .expect("BUG: a changed owner is observed");
        assert!(
            registry
                .observe(Target::Window(identity.hwnd, replacement), || Ok(()))
                .is_err()
        );
    }

    #[test]
    fn activation_refused_before_start_has_no_effect() {
        let began = std::cell::Cell::new(false);
        let outcome = activate_while_current(
            || Err(no_window(Target::Window(7, 1))),
            || {
                began.set(true);
                Ok(())
            },
        );
        let error = outcome.expect_err("BUG: the target is already gone");
        assert!(!began.get());
        assert_eq!(error.payload()["error"]["code"], "gone");
        assert!(error.payload()["error"].get("effect").is_none());
    }

    #[test]
    fn activation_rechecks_identity_on_success_and_failure() {
        for backend_succeeds in [false, true] {
            let mut registry = Registry::default();
            let issued = Issued {
                hwnd: 7,
                pid: 100,
                started: Some(123),
                class: Some(1),
            };
            let handle = registry.register_window(issued);
            let target = Target::Window(issued.hwnd, handle);
            let alive = std::cell::Cell::new(true);
            let outcome = activate_while_current(
                || {
                    registry.observe(target, || {
                        if alive.get() {
                            Ok(())
                        } else {
                            Err(no_window(target))
                        }
                    })
                },
                || {
                    alive.set(false);
                    if backend_succeeds {
                        Ok(())
                    } else {
                        Err(ToolError::NotFound(
                            "window vanished before it could be raised".into(),
                        ))
                    }
                },
            );
            let error = outcome.expect_err("BUG: closed window cannot be activated");
            assert_eq!(error.payload()["error"]["code"], "gone");
            assert_eq!(error.payload()["error"]["kind"], "window");
            assert_eq!(error.payload()["error"]["effect"]["kind"], "may_have_run");
            alive.set(true);
            assert!(registry.observe(target, || Ok(())).is_err());
        }
    }

    #[test]
    fn a_replaced_window_handle_stays_gone_when_its_identity_returns() {
        let mut registry = Registry::default();
        let original = Issued {
            hwnd: 7,
            pid: 100,
            started: Some(123),
            class: Some(1),
        };
        let first = registry.register_window(original);
        assert_eq!(registry.register_window(original), first);
        let replacement = registry.register_window(Issued {
            class: Some(2),
            ..original
        });
        assert_ne!(replacement, first);
        let returned = registry.register_window(original);
        assert_ne!(returned, first);
        assert_ne!(returned, replacement);
        assert_eq!(registry.register_window(original), returned);
        for stale in [first, replacement] {
            assert!(
                registry
                    .observe(Target::Window(original.hwnd, stale), || Ok(()))
                    .is_err(),
                "even a matching current identity cannot revive a displaced handle"
            );
            let error = registry
                .bound(TargetArg::Window(stale))
                .expect_err("BUG: a displaced handle never revives");
            assert_eq!(error.payload()["error"]["code"], "gone");
            assert_eq!(error.payload()["error"]["handle"], format!("w{stale}"));
        }
        assert_eq!(registry.by_hwnd.get(&original.hwnd), Some(&returned));
    }

    #[test]
    fn a_failed_capture_rechecks_identity_and_never_revives_a_closed_handle() {
        let registry = Registry::default();
        let alive = std::cell::Cell::new(true);
        let target = Target::Window(7, 1);
        let result = read_while_current(
            || {
                registry.observe(target, || {
                    if alive.get() {
                        Ok(())
                    } else {
                        Err(no_window(target))
                    }
                })
            },
            || {
                alive.set(false);
                Err::<(), _>(ToolError::NotFound("capture lost its window".into()))
            },
        );
        let error = result.expect_err("BUG: the window closed during capture");
        assert_eq!(error.payload()["error"]["code"], "gone");
        assert_eq!(error.payload()["error"]["kind"], "window");
        alive.set(true);
        assert!(
            registry.observe(target, || Ok(())).is_err(),
            "a matching native id cannot revive w1"
        );
        assert!(
            registry.observe(Target::Window(7, 2), || Ok(())).is_ok(),
            "a replacement has its own handle"
        );
    }

    #[test]
    fn a_capture_error_does_not_retire_a_live_window() {
        let registry = Registry::default();
        let target = Target::Window(7, 1);
        let result = read_while_current(
            || registry.observe(target, || Ok(())),
            || {
                Err::<(), _>(ToolError::NotFound(
                    "popup is absent from capture list".into(),
                ))
            },
        );
        assert!(matches!(result, Err(ToolError::NotFound(_))));
        assert!(registry.observe(target, || Ok(())).is_ok());
    }

    fn fg() -> Foreground {
        Foreground {
            id: 10,
            pid: 100,
            handle: Some("w1".into()),
            title: Some("App".into()),
            rect: Some(Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            }),
        }
    }

    fn name(hwnd: u32, pid: u32) -> WindowRef {
        WindowRef {
            window: (hwnd == 10).then(|| "w1".into()),
            pid,
            title: None,
        }
    }

    const OWN: Under = Under {
        id: 10,
        pid: 100,
        inner_pid: 100,
    };

    const WINDOW: Target = Target::Window(10, 1);

    #[test]
    fn refuses_when_nothing_or_something_else_is_in_front() {
        assert!(matches!(
            verify(Target::Pid(100), None, &[], &name),
            Err(ToolError::NotForeground { .. })
        ));
        assert!(matches!(
            verify(Target::Window(11, 2), Some(&fg()), &[], &name),
            Err(ToolError::NotForeground { .. })
        ));
        let err = verify(Target::Pid(101), Some(&fg()), &[], &name).expect_err("BUG: refused");
        assert!(
            matches!(&err, ToolError::NotForeground { foreground: Some(f), .. } if f.window.as_deref() == Some("w1")),
            "the foreground is named by handle: {err}"
        );
    }

    #[test]
    fn accepts_the_foreground_target_by_window_or_pid() {
        assert!(verify(WINDOW, Some(&fg()), &[], &name).is_ok());
        assert!(verify(Target::Pid(100), Some(&fg()), &[((5, 5), Some(OWN))], &name).is_ok());
        assert!(verify(WINDOW, Some(&fg()), &[((5, 5), Some(OWN))], &name).is_ok());
    }

    /// A pid held by another process than the one first seen under it, or
    /// by none, is gone; so is one whose process identity the OS cannot
    /// report, which could not be told from a reuse.
    #[test]
    fn a_recycled_pid_is_gone() {
        assert!(same_process(7, Some(1), Some(1)).is_ok());
        let err = same_process(7, Some(1), Some(2)).expect_err("BUG: another process");
        assert!(
            matches!(
                &err,
                ToolError::Gone {
                    kind: HandleKind::Process,
                    ..
                }
            ),
            "{err}"
        );
        assert!(err.to_string().contains("another process"), "{err}");
        assert!(
            same_process(7, Some(1), None).is_err(),
            "the process exited"
        );
        let err = same_process(7, None, Some(2)).expect_err("BUG: no identity to hold");
        assert!(matches!(err, ToolError::NotSupported(_)), "{err}");
    }

    /// A point whose window the OS cannot name is refused, not waved
    /// through: on an OS without hit-testing a covering window would
    /// otherwise take the input.
    #[test]
    fn a_point_with_nothing_known_under_it_fails_closed() {
        assert!(matches!(
            verify(Target::Pid(100), Some(&fg()), &[((5, 5), None)], &name),
            Err(ToolError::OutsideTarget { .. })
        ));
    }

    /// A window target admits only that window at the point: a sibling
    /// window of the same process in front of it would take the click.
    #[test]
    fn a_window_target_refuses_its_processes_other_windows() {
        let sibling = Under { id: 12, ..OWN };
        assert!(matches!(
            verify(WINDOW, Some(&fg()), &[((5, 5), Some(sibling))], &name),
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
        let err = verify(
            Target::Pid(100),
            Some(&fg()),
            &[((5, 5), Some(covered))],
            &name,
        )
        .expect_err("BUG: a covered point must be refused");
        assert!(err.to_string().contains("covered"), "{err}");
        assert!(
            matches!(&err, ToolError::OutsideTarget { covered_by: Some(w), .. } if w.pid == 555),
            "the covering window is data: {err}"
        );
        let own_popup = Under { id: 12, ..OWN };
        assert!(
            verify(
                Target::Pid(100),
                Some(&fg()),
                &[((5, 5), Some(own_popup))],
                &name
            )
            .is_ok()
        );
    }

    /// Another process's child window inside the target (a preview pane)
    /// takes the click itself, so the point is refused for either target.
    #[test]
    fn a_hosted_window_of_another_process_is_refused() {
        let hosted = Under {
            inner_pid: 555,
            ..OWN
        };
        for target in [WINDOW, Target::Pid(100)] {
            let err = verify(target, Some(&fg()), &[((5, 5), Some(hosted))], &name)
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
        // Its own error: "activate the window" would not move the focus.
        assert!(
            matches!(err, ToolError::FocusElsewhere { holder: 555, .. }),
            "{err}"
        );
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

    /// A handle this session never issued is unknown, not a missing window
    /// that `wait_for` would keep polling for.
    #[test]
    fn an_unissued_target_is_unknown() {
        let mut registry = Registry::default();
        // Where the OS reports no start times, a pid is no target at all,
        // and that is the answer instead.
        let pid = registry.bound(TargetArg::Pid(std::process::id()));
        if cfg!(target_os = "windows") {
            assert!(matches!(
                pid,
                Err(ToolError::UnknownHandle {
                    kind: HandleKind::Process,
                    ..
                })
            ));
        } else {
            assert!(matches!(pid, Err(ToolError::NotSupported(_))));
        }
        assert!(matches!(
            registry.bound(TargetArg::Window(123_456_789)),
            Err(ToolError::UnknownHandle {
                kind: HandleKind::Window,
                ..
            })
        ));
    }

    /// The safety wiring end to end, on any host: a shell hotkey with a
    /// target is refused before anything else is looked at, and handles and
    /// pids this session never issued are refused.
    #[test]
    fn a_desktop_refuses_what_it_cannot_bind() {
        let mut desktop = Desktop::new();
        if desktop.input().is_ok() {
            let combo = KeyCombo::parse("win+r").expect("BUG: parses");
            let err = desktop
                .key(&combo, 1, TargetArg::Window(1))
                .expect_err("BUG: must be refused")
                .to_string();
            assert!(err.contains("meta+r"), "{err}");
            let err = desktop
                .type_text("x", TargetArg::Window(123_456_789))
                .expect_err("BUG: must be refused");
            assert_eq!(err.code(), "unknown_handle", "{err}");
            let err = desktop
                .type_text("x", TargetArg::Pid(u32::MAX - 7))
                .expect_err("BUG: must be refused");
            assert!(
                err.code() == "unknown_handle" || err.code() == "not_supported",
                "{err}"
            );
        } else {
            // No input device on this OS: that is the reason reported, not
            // a binding error about ids that could never have been listed.
            let err = desktop
                .type_text("x", TargetArg::Window(1))
                .expect_err("BUG: must be refused")
                .to_string();
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
        assert!(verify_element(20, 100, Some(&fg()), (5, 5), Some(menu), &name).is_ok());
        assert!(matches!(
            verify_element(20, 100, Some(&fg()), (5, 5), Some(OWN), &name),
            Err(ToolError::OutsideTarget { .. })
        ));
        let other_app = Foreground { pid: 555, ..fg() };
        assert!(matches!(
            verify_element(20, 100, Some(&other_app), (5, 5), Some(menu), &name),
            Err(ToolError::NotForeground { .. })
        ));
        assert!(verify_element(20, 100, Some(&fg()), (5, 5), None, &name).is_err());
    }

    /// Screenshot handles stay addressable for the last few shots; an
    /// older one is gone, an unissued one unknown.
    #[test]
    fn screenshot_handles_expire_in_order() {
        let mut registry = Registry::default();
        let meta = ShotMeta {
            source: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            scale_x: 1.0,
            scale_y: 1.0,
            width: 10,
            height: 10,
            window: None,
            monitor: None,
        };
        let first = registry.remember_shot(meta);
        assert_eq!(first, "s1");
        for _ in 0..SHOTS_KEPT {
            registry.remember_shot(meta);
        }
        assert!(matches!(registry.shot(1), Err(ToolError::Gone { .. })));
        assert!(registry.shot(2).is_ok());
        assert!(matches!(
            registry.shot(99),
            Err(ToolError::UnknownHandle { .. })
        ));
    }
}
