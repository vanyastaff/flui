//! The desktop session: accessibility backend, input device and window
//! lookups, owned by the worker thread and called one request at a time.
//!
//! Every input method that takes a safety target checks it immediately
//! before sending the input: the target must own the foreground window, and a
//! coordinate must lie inside that window and not under another window.

use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::a11y::{self, AccessibilityBackend, Action, Node, Query};
use crate::capture::{self, Shot, ShotTarget, WindowInfo};
use crate::error::{ToolError, ToolResult};
use crate::geometry::Rect;
use crate::input::{Input, MouseButton};
use crate::keys::KeyCombo;
use crate::params::{ClickAt, ScreenshotTarget, Target};

/// The foreground window as the safety check sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Foreground {
    /// Window id.
    pub id: u32,
    /// Owning process.
    pub pid: u32,
    /// Title, when the window list has it.
    pub title: Option<String>,
    /// Bounds, when the window list has them.
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

/// The owner of the top-level window under a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Under {
    /// Window id.
    pub id: u32,
    /// Owning process.
    pub pid: u32,
}

/// Refuses input unless `target` owns the foreground window and every point
/// lies inside it, uncovered. `under` gives, per point, the window the OS
/// says is there (`None` where the OS cannot say).
pub fn verify(
    target: Target,
    foreground: Option<&Foreground>,
    points: &[((i32, i32), Option<Under>)],
) -> ToolResult<()> {
    let Some(fg) = foreground else {
        return Err(ToolError::NotForeground {
            target: target.to_string(),
            foreground: "none".into(),
        });
    };
    let owns = match target {
        Target::Window(id) => fg.id == id,
        Target::Pid(pid) => fg.pid == pid,
    };
    if !owns {
        return Err(ToolError::NotForeground {
            target: target.to_string(),
            foreground: fg.to_string(),
        });
    }
    for &((x, y), under) in points {
        let Some(rect) = fg.rect else {
            return Err(ToolError::NotFound(format!(
                "cannot verify ({x}, {y}): the foreground window's bounds are unknown"
            )));
        };
        if !rect.contains(x, y) {
            return Err(ToolError::OutsideTarget {
                x,
                y,
                rect: rect.to_string(),
            });
        }
        if let Some(under) = under
            && under.pid != fg.pid
        {
            return Err(ToolError::OutsideTarget {
                x,
                y,
                rect: format!(
                    "{rect}, covered there by window {} of process {}",
                    under.id, under.pid
                ),
            });
        }
    }
    Ok(())
}

/// Session state on the worker thread.
pub struct Desktop {
    a11y: Box<dyn AccessibilityBackend>,
    input: Result<Input, String>,
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
        }
    }

    fn input(&mut self) -> ToolResult<&mut Input> {
        self.input
            .as_mut()
            .map_err(|e| ToolError::NotSupported(e.clone()))
    }

    /// The window list, optionally filtered.
    pub fn list_windows(
        title_contains: Option<&str>,
        pid: Option<u32>,
    ) -> ToolResult<Vec<WindowInfo>> {
        let needle = title_contains.map(str::to_lowercase);
        Ok(capture::windows()?
            .into_iter()
            .filter(|w| pid.is_none_or(|p| w.pid == p))
            .filter(|w| {
                needle
                    .as_deref()
                    .is_none_or(|n| w.title.to_lowercase().contains(n))
            })
            .collect())
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
            return Err(ToolError::NotFound(match target {
                Target::Window(id) => format!("no window with id {id}; call list_windows"),
                Target::Pid(pid) => format!(
                    "process {pid} has no top-level window; if the app hands off to another \
                     process (Windows 11 Notepad does), find it with list_windows and pass window_id"
                ),
            }));
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

    /// The element trees of a target's windows.
    pub fn tree(&mut self, target: Target, max_depth: usize) -> ToolResult<Vec<Node>> {
        let ids: Vec<u32> = Self::resolve(target)?.iter().map(|w| w.id).collect();
        self.a11y.tree(&ids, max_depth)
    }

    /// Elements matching `query` anywhere in a target's windows.
    pub fn find(&mut self, target: Target, query: &Query) -> ToolResult<(Vec<Node>, Vec<Node>)> {
        let roots = self.tree(target, usize::MAX)?;
        Ok((a11y::search(&roots, query), roots))
    }

    /// Performs a pattern action.
    pub fn act(&mut self, element: &str, action: &Action) -> ToolResult<Node> {
        self.a11y.act(element, action)
    }

    fn foreground() -> ToolResult<Option<Foreground>> {
        let windows = capture::windows()?;
        Ok(match crate::os::foreground() {
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

    fn check(target: Option<Target>, points: &[(i32, i32)]) -> ToolResult<()> {
        let Some(target) = target else {
            return Ok(());
        };
        let fg = Self::foreground()?;
        let points: Vec<_> = points
            .iter()
            .map(|&(x, y)| {
                let under = crate::os::window_at(x, y).map(|(id, pid)| Under { id, pid });
                ((x, y), under)
            })
            .collect();
        verify(target, fg.as_ref(), &points)
    }

    /// Clicks an element's clickable point or a screen point.
    pub fn click(
        &mut self,
        at: &ClickAt,
        button: MouseButton,
        double: bool,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        let (x, y, element_pid) = match at {
            ClickAt::Point(x, y) => (*x, *y, None),
            ClickAt::Element(handle) => {
                let p = self.a11y.click_point(handle)?;
                (p.x, p.y, Some(p.pid))
            }
        };
        Self::check(target, &[(x, y)])?;
        // An element click always lands in the element's own process: the
        // point must be in its foreground window, not on whatever covers it.
        if let Some(pid) = element_pid {
            Self::check(Some(Target::Pid(pid)), &[(x, y)])?;
        }
        self.input()?.click(x, y, button, double)?;
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
        Self::check(target, &[from, to])?;
        self.input()?.drag(from, to, duration)?;
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
        Self::check(target, &[(x, y)])?;
        self.input()?.scroll(x, y, dx, dy)?;
        Ok(json!({ "at": { "x": x, "y": y }, "dx": dx, "dy": dy }))
    }

    /// Types text into whatever has keyboard focus.
    pub fn type_text(&mut self, text: &str, target: Option<Target>) -> ToolResult<Value> {
        Self::check(target, &[])?;
        self.input()?.type_text(text)?;
        Ok(json!({ "typed_chars": text.chars().count() }))
    }

    /// Presses a key combo.
    pub fn key(
        &mut self,
        combo: &KeyCombo,
        repeat: u32,
        target: Option<Target>,
    ) -> ToolResult<Value> {
        Self::check(target, &[])?;
        self.input()?.key(combo, repeat)?;
        Ok(json!({ "pressed": combo.to_string(), "repeat": repeat }))
    }

    /// Brings a window to the front and reports whether it got there.
    pub fn activate(&mut self, target: Target) -> ToolResult<Value> {
        let windows = Self::resolve(target)?;
        let window = windows
            .iter()
            .find(|w| !w.is_minimized)
            .unwrap_or(&windows[0])
            .clone();
        let mut attempts = Vec::new();
        crate::os::bring_to_front(window.id)?;
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
        assert!(verify(Target::Pid(100), Some(&fg()), &[((5, 5), None)]).is_ok());
    }

    #[test]
    fn refuses_points_outside_or_covered() {
        assert!(matches!(
            verify(Target::Pid(100), Some(&fg()), &[((800, 5), None)]),
            Err(ToolError::OutsideTarget { .. })
        ));
        let covered = Under { id: 99, pid: 555 };
        let err = verify(Target::Pid(100), Some(&fg()), &[((5, 5), Some(covered))])
            .expect_err("BUG: a covered point must be refused");
        assert!(err.to_string().contains("covered"), "{err}");
        let own_popup = Under { id: 12, pid: 100 };
        assert!(verify(Target::Pid(100), Some(&fg()), &[((5, 5), Some(own_popup))]).is_ok());
    }

    #[test]
    fn unknown_bounds_fail_closed_for_points_only() {
        let no_rect = Foreground { rect: None, ..fg() };
        assert!(verify(Target::Pid(100), Some(&no_rect), &[]).is_ok());
        assert!(verify(Target::Pid(100), Some(&no_rect), &[((1, 1), None)]).is_err());
    }
}
