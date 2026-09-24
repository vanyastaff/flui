//! The desktop session: accessibility backend, input device and window
//! lookups, owned by the worker thread and called one request at a time.
//!
//! Every input method that takes a safety target checks it immediately
//! before sending the input: the target must own the foreground window, and a
//! coordinate must be where the OS says the target's window is, uncovered.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use crate::a11y::{self, AccessibilityBackend, Action, Node, Query, Read};
use crate::capture::{self, Shot, ShotTarget, WindowInfo};
use crate::error::{ToolError, ToolResult};
use crate::geometry::Rect;
use crate::input::{Input, MouseButton};
use crate::keys::KeyCombo;
use crate::os::{self, Under};
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
        rect: format!("{what}; the point is covered by {by}"),
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
/// it (`then`, its start time): Windows recycles pids, and input must not
/// follow a pid to an unrelated process. `now` is the start time of the
/// process holding the pid now, `None` when none does.
pub fn same_process(pid: u32, then: Option<u64>, now: Option<u64>) -> ToolResult<()> {
    match then {
        Some(then) if now != Some(then) => Err(ToolError::NotFound(format!(
            "process {pid} has exited since this session saw it{}; list_windows or launch again",
            if now.is_some() {
                " (the pid now belongs to another process)"
            } else {
                ""
            }
        ))),
        _ => Ok(()),
    }
}

/// Session state on the worker thread.
pub struct Desktop {
    a11y: Box<dyn AccessibilityBackend>,
    input: Result<Input, String>,
    /// Every window id this session handed out, with the process that owned
    /// it then. Windows recycles an `HWND` for an unrelated window, so a
    /// window target is accepted only while the same process still owns it.
    issued: HashMap<u32, u32>,
    /// Every pid this session saw, with that process's start time (`None`
    /// where the OS does not report it): a pid target is accepted only while
    /// the same process holds the pid.
    started: HashMap<u32, Option<u64>>,
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

    /// Records the process holding `pid` now, unless the pid is already
    /// bound: the first sighting is the one later targets are held to.
    pub fn adopt_pid(&mut self, pid: u32) {
        self.started
            .entry(pid)
            .or_insert_with(|| os::process_started(pid));
    }

    /// Records a window id with its owner, unless the id is already bound.
    fn adopt_window(&mut self, w: &WindowInfo) {
        self.issued.entry(w.id).or_insert(w.pid);
        self.adopt_pid(w.pid);
    }

    /// The window list, optionally filtered. Every id and pid it returns is
    /// recorded with its owner, so a later target can be bound to it.
    pub fn list_windows(
        &mut self,
        title_contains: Option<&str>,
        pid: Option<u32>,
    ) -> ToolResult<Vec<WindowInfo>> {
        let needle = title_contains.map(str::to_lowercase);
        let windows: Vec<WindowInfo> = capture::windows()?
            .into_iter()
            .filter(|w| pid.is_none_or(|p| w.pid == p))
            .filter(|w| {
                needle
                    .as_deref()
                    .is_none_or(|n| w.title.to_lowercase().contains(n))
            })
            .collect();
        for w in &windows {
            // A listing re-binds: the list is the fresh sighting an agent
            // is told to take after a recycled id or pid was refused.
            self.issued.insert(w.id, w.pid);
            self.started.insert(w.pid, os::process_started(w.pid));
        }
        Ok(windows)
    }

    /// The process a window target is bound to: the owner recorded when this
    /// session handed the id out. An id never handed out is refused — it
    /// cannot be told apart from a recycled one. A pid target is held to the
    /// process first seen under it.
    fn bound(&mut self, target: Option<Target>) -> ToolResult<Option<u32>> {
        match target {
            Some(Target::Window(id)) => self.issued.get(&id).copied().map(Some).ok_or_else(|| {
                ToolError::NotFound(format!(
                    "window {id} was not listed in this session; take window ids from list_windows or launch"
                ))
            }),
            Some(Target::Pid(pid)) => {
                let now = os::process_started(pid);
                let then = *self.started.entry(pid).or_insert(now);
                same_process(pid, then, now)?;
                Ok(None)
            }
            None => Ok(None),
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
    pub fn screenshot(target: ScreenshotTarget, max_side: Option<u32>) -> ToolResult<Shot> {
        let direct = match target {
            ScreenshotTarget::Direct(t) => t,
            ScreenshotTarget::Pid(pid) => {
                let windows = Self::resolve(Target::Pid(pid))?;
                let pick = windows
                    .iter()
                    .find(|w| w.is_focused)
                    .or_else(|| windows.iter().find(|w| !w.is_minimized))
                    .unwrap_or(&windows[0]);
                ShotTarget::Window(pick.id)
            }
        };
        capture::screenshot(direct, max_side)
    }

    /// The element trees of a target's windows. A process's popups (menus,
    /// drop-downs) are windows of their own and are read too where the OS
    /// lists them.
    pub fn tree(
        &mut self,
        target: Target,
        max_depth: usize,
        deadline: Instant,
    ) -> ToolResult<Read> {
        let ids: Vec<u32> = match target {
            Target::Pid(pid) => match os::process_windows(pid) {
                Some(ids) if !ids.is_empty() => ids,
                Some(_) => return Err(no_window(target)),
                None => Self::resolve(target)?.iter().map(|w| w.id).collect(),
            },
            Target::Window(_) => Self::resolve(target)?.iter().map(|w| w.id).collect(),
        };
        self.a11y.tree(&ids, max_depth, deadline)
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

    fn check(target: Option<Target>, bound: Option<u32>, points: &[(i32, i32)]) -> ToolResult<()> {
        let Some(target) = target else {
            return Ok(());
        };
        let fg = Self::foreground()?;
        still_bound(target, bound, fg.as_ref())?;
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
                (p.x, p.y, Some((window, pid)))
            }
        };
        let mut guard = |_: Option<(i32, i32)>| {
            Self::check(target, bound, &[(x, y)])?;
            // An element click always lands on the element: the point must
            // be in its window, with its application in front.
            element_window.map_or(Ok(()), |(window, pid)| {
                Self::check_element(window, pid, (x, y))
            })
        };
        guard(None)?;
        self.input()?.click(x, y, button, double, &mut guard)?;
        Ok(
            json!({ "clicked": { "x": x, "y": y }, "button": format!("{button:?}").to_lowercase(), "double": double }),
        )
    }

    /// Moves the pointer.
    pub fn move_mouse(&mut self, x: i32, y: i32) -> ToolResult<Value> {
        let input = self.input()?;
        input.move_to(x, y)?;
        std::thread::sleep(Duration::from_millis(10));
        let at = input.position();
        Ok(
            json!({ "requested": { "x": x, "y": y }, "pointer": at.map(|(x, y)| json!({ "x": x, "y": y })) }),
        )
    }

    /// Drags with the left button.
    pub fn drag(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        duration: Duration,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        let bound = self.bound(target)?;
        Self::check(target, bound, &[from, to])?;
        let mut guard = |at: Option<(i32, i32)>| {
            let mut points = vec![from, to];
            points.extend(at);
            Self::check(target, bound, &points)
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
        let bound = self.bound(target)?;
        Self::check(target, bound, &[(x, y)])?;
        let mut guard = |_: Option<(i32, i32)>| Self::check(target, bound, &[(x, y)]);
        self.input()?.scroll(x, y, dx, dy, &mut guard)?;
        Ok(json!({ "at": { "x": x, "y": y }, "dx": dx, "dy": dy }))
    }

    /// Types text into whatever has keyboard focus.
    pub fn type_text(&mut self, text: &str, target: Option<Target>) -> ToolResult<Value> {
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
        if target.is_some()
            && let Some(handler) = combo.shell_hotkey(cfg!(target_os = "macos"))
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
        let bound = self.bound(Some(target))?;
        let windows = Self::resolve(target)?;
        if let (Some(pid), Some(w)) = (bound, windows.first())
            && w.pid != pid
        {
            return Err(ToolError::NotFound(format!(
                "window {} belonged to process {pid} when listed and now belongs to process {}; list_windows again",
                w.id, w.pid
            )));
        }
        for w in &windows {
            self.adopt_window(w);
        }
        let window = windows
            .iter()
            .find(|w| !w.is_minimized)
            .unwrap_or(&windows[0])
            .clone();
        let mut attempts = Vec::new();
        os::bring_to_front(window.id)?;
        attempts.push("SetForegroundWindow");
        let mut fg = Self::foreground()?;
        if fg.as_ref().map(|f| f.id) != Some(window.id) {
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
    /// by none, is refused; an OS that reports no start times binds nothing.
    #[test]
    fn a_recycled_pid_is_refused() {
        assert!(same_process(7, Some(1), Some(1)).is_ok());
        let err = same_process(7, Some(1), Some(2)).expect_err("BUG: another process");
        assert!(err.to_string().contains("another process"), "{err}");
        assert!(
            same_process(7, Some(1), None).is_err(),
            "the process exited"
        );
        assert!(same_process(7, None, Some(2)).is_ok());
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
