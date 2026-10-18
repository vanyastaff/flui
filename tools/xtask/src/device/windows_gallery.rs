//! `windows-gallery`: the interaction gallery on its real Windows window,
//! every screen driven by real input and each outcome read twice.
//!
//! The input is what a user has: `SendInput` mouse clicks, drags, wheel
//! notches and keys, and touch contacts from a synthetic touchscreen
//! (`InjectSyntheticPointerInput`), all hit-tested and routed by the OS.
//! After each action the check reads the outcome through UI Automation —
//! the screen's readout text, a row gone, a menu open, the element with the
//! keyboard focus — and through the window's pixels, sampled where the
//! right outcome and the wrong one paint different colours: the sheet's top
//! edge at its anchor, the grid cell under a point after a zoom, a revealed
//! swipe action.
//!
//! Every screen is opened from the sidebar, which builds it afresh, so one
//! screen's failure leaves the next with its initial state. Each check is
//! reported as it finishes; the run fails if any did. A host that refuses
//! the input or the foreground cannot verify (exit 2), and no further input
//! is sent.

mod checks;

use std::cell::Cell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Accessibility::{
    IUIAutomationElement, UIA_CONTROLTYPE_ID, UIA_EditControlTypeId, UIA_MenuControlTypeId,
    UIA_MenuItemControlTypeId, UIA_SliderControlTypeId,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY, VK_APPS, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowInfo, GetWindowTextW, GetWindowThreadProcessId,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursorPos, SetForegroundWindow,
    SetWindowPos, WINDOWINFO,
};

use super::uia::{self, BUTTON, Session, Start, TEXT};
use super::windows_input::{self as input, Cursor};
use super::windows_screen::{Capture, Rgb, Touchscreen, touch_refused_by_host};

/// How long one observation may take to come true.
const WITHIN: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(60);
/// Between the events of one action.
const HOLD: Duration = Duration::from_millis(40);
/// How long something that must not change is watched.
const STILL: Duration = Duration::from_millis(600);
/// About how long a run drives the desktop, as it announces.
const RUN_MINUTES: u32 = 4;
/// Narrows a run to the checks whose name contains its value, such as
/// `focus:`; unset or empty runs every check.
const ONLY_ENV: &str = "FLUI_GALLERY_CHECKS";
/// The sidebar entry the gallery opens on, which proves its window is up.
const FIRST_SCREEN: &str = "Swipe actions";

/// Why a run stops sending input: the host refused it.
#[derive(Debug)]
struct CannotVerify(String);

impl fmt::Display for CannotVerify {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CannotVerify {}

fn host(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CannotVerify(message.into()))
}

/// Exit 0 when every check passed, 1 when any failed, 2 when the host
/// refused the measurement. Captures land in `shots`.
pub(super) fn run(gallery: &Path, shots: &Path) -> anyhow::Result<u8> {
    println!(
        "windows-gallery: this takes the mouse, the keyboard and touch input for about {RUN_MINUTES} minutes; do not use them until it ends"
    );
    // SAFETY: process-local, before this process creates any window.
    if let Err(error) =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
    {
        println!("GALLERY=CANNOT_VERIFY: per-monitor DPI awareness was refused: {error}");
        return Ok(2);
    }
    let mut session = match Session::start(gallery) {
        Ok(session) => session,
        Err(Start::CannotVerify(why)) => {
            println!("GALLERY=CANNOT_VERIFY: {why}");
            return Ok(2);
        }
        Err(Start::Failed(error)) => return Err(error),
    };
    let cursor = Cursor::save();
    let outcome = drive(&mut session, shots);
    drop(cursor);
    drop(session);
    match outcome {
        Ok(report) => Ok(report.verdict()),
        Err(error) if error.downcast_ref::<CannotVerify>().is_some() => {
            println!("GALLERY=CANNOT_VERIFY: {error:#}");
            Ok(2)
        }
        Err(error) => {
            println!("GALLERY=FAIL: {error:#}");
            Ok(1)
        }
    }
}

/// What each check came to.
#[derive(Default)]
struct Report {
    rows: Vec<(String, Result<(), String>)>,
}

impl Report {
    fn record(&mut self, name: &str, outcome: anyhow::Result<()>) -> anyhow::Result<()> {
        let outcome = match outcome {
            Ok(()) => Ok(()),
            // A refusal ends the run: no further input may be sent.
            Err(error) if error.downcast_ref::<CannotVerify>().is_some() => return Err(error),
            Err(error) => Err(format!("{error:#}")),
        };
        match &outcome {
            Ok(()) => println!("CHECK PASS {name}"),
            Err(why) => println!("CHECK FAIL {name}: {why}"),
        }
        self.rows.push((name.to_owned(), outcome));
        Ok(())
    }

    fn verdict(&self) -> u8 {
        println!("windows-gallery summary:");
        for (name, outcome) in &self.rows {
            match outcome {
                Ok(()) => println!("  PASS  {name}"),
                Err(why) => println!("  FAIL  {name}: {why}"),
            }
        }
        let failed = self
            .rows
            .iter()
            .filter(|(_, outcome)| outcome.is_err())
            .count();
        if failed == 0 {
            println!("GALLERY=PASS ({} checks)", self.rows.len());
            0
        } else {
            println!("GALLERY=FAIL ({failed} of {} checks)", self.rows.len());
            1
        }
    }
}

fn drive(session: &mut Session, shots: &Path) -> anyhow::Result<Report> {
    let window = session
        .window_with_button(FIRST_SCREEN)?
        .ok_or_else(|| anyhow::anyhow!("the gallery never published its sidebar"))?;
    // SAFETY: current properties on the session's COM thread.
    let (hwnd, pid) = unsafe {
        (
            window.CurrentNativeWindowHandle()?,
            window.CurrentProcessId()?,
        )
    };
    let touch = match Touchscreen::new(2) {
        Ok(touch) => Some(touch),
        Err(error) if touch_refused_by_host(&error) => {
            println!(
                "touch: this host refuses synthetic touch ({error}); touch checks cannot verify"
            );
            None
        }
        Err(error) => return Err(error.into()),
    };
    let driver = Driver {
        session,
        window,
        hwnd,
        pid: u32::try_from(pid)?,
        touch,
        shots: shots.to_path_buf(),
        lost_clicks: Cell::new(0),
        refocused: Cell::new(0),
    };
    // Windows lets the process that sent the last input event take the
    // foreground: a motionless mouse move makes this one that process, so
    // the gallery it launched can be brought forward without the operator.
    input::send(&[mouse(MOUSEEVENTF_MOVE, 0)])
        .map_err(|error| host(format!("input refused: {error:#}")))?;
    if input::foreground(driver.session, driver.hwnd).is_none() {
        return Err(host("Windows refused the gallery the foreground"));
    }
    // Topmost for the run: a window that takes the foreground between the
    // hit check and the press (a notification raising itself) can then not
    // cover the gallery and take the click. The gallery's own window, which
    // the run kills at its end.
    // SAFETY: a z-order change of the launched process's live window; no
    // move, no resize, no activation.
    unsafe {
        SetWindowPos(
            driver.hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    }
    .map_err(|error| host(format!("the gallery could not be kept on top: {error}")))?;
    // Let the activation reach the window before the first click.
    std::thread::sleep(Duration::from_millis(500));
    let mut report = Report::default();
    let mut shot: Vec<&str> = Vec::new();
    let only = std::env::var(ONLY_ENV).ok().filter(|only| !only.is_empty());
    for (name, check) in checks::CHECKS {
        if only.as_deref().is_some_and(|only| !name.contains(only)) {
            continue;
        }
        let outcome = driver.open(check.screen).and_then(|()| {
            // Each screen as it first opens, for a person to look at.
            if !shot.contains(&check.screen) {
                shot.push(check.screen);
                driver.shoot(&check.screen.to_lowercase().replace(' ', "-"))?;
            }
            (check.run)(&driver)
        });
        if outcome.is_err() {
            println!("tree after {name}:");
            driver.dump();
        }
        report.record(name, outcome)?;
    }
    if driver.lost_clicks.get() > 0 {
        println!(
            "NOTE: {} sidebar clicks did nothing and were retried",
            driver.lost_clicks.get()
        );
    }
    if driver.refocused.get() > 0 {
        println!(
            "NOTE: another window took the foreground {} times and gave it back",
            driver.refocused.get()
        );
    }
    Ok(report)
}

/// One check: the screen it opens and what it does there.
pub(super) struct Check {
    pub(super) screen: &'static str,
    pub(super) run: fn(&Driver<'_>) -> anyhow::Result<()>,
}

/// The gallery's window, and the input and observations a check uses.
pub(super) struct Driver<'a> {
    session: &'a Session,
    window: IUIAutomationElement,
    hwnd: HWND,
    pid: u32,
    touch: Option<Touchscreen>,
    shots: PathBuf,
    /// Sidebar clicks that changed nothing and were retried.
    lost_clicks: Cell<u32>,
    /// Times another window took the foreground and gave it back.
    refocused: Cell<u32>,
}

/// The UIA control types the checks look for, beside `uia`'s button and
/// text.
pub(super) const EDIT: UIA_CONTROLTYPE_ID = UIA_EditControlTypeId;
pub(super) const MENU: UIA_CONTROLTYPE_ID = UIA_MenuControlTypeId;
pub(super) const MENU_ITEM: UIA_CONTROLTYPE_ID = UIA_MenuItemControlTypeId;
pub(super) const SLIDER: UIA_CONTROLTYPE_ID = UIA_SliderControlTypeId;

impl Driver<'_> {
    // ---------------------------------------------------------------------
    // Identity and geometry
    // ---------------------------------------------------------------------

    fn owned(&self) -> anyhow::Result<()> {
        let mut pid = 0;
        // SAFETY: a read-only ownership query on the gallery's handle.
        let thread = unsafe { GetWindowThreadProcessId(self.hwnd, Some(&raw mut pid)) };
        if thread == 0 || pid != self.pid || !self.session.owns_window(self.hwnd) {
            return Err(host(
                "the gallery's window no longer belongs to the launched process",
            ));
        }
        Ok(())
    }

    /// Every event goes to the gallery only: it must be the foreground
    /// window. Another window that took the foreground between two actions
    /// is asked to give it back once (the check is the process sending
    /// input, which Windows lets take the foreground); if it does not, no
    /// further input is sent.
    fn before_input(&self) -> anyhow::Result<()> {
        self.owned()?;
        // SAFETY: reads foreground state only.
        if unsafe { GetForegroundWindow() } == self.hwnd {
            return Ok(());
        }
        let thief = foreground_window();
        self.refocused.set(self.refocused.get() + 1);
        println!("NOTE: {thief} took the foreground; bringing the gallery back");
        input::send(&[mouse(MOUSEEVENTF_MOVE, 0)])
            .map_err(|error| host(format!("input refused: {error:#}")))?;
        if acquire_foreground(self.hwnd) && input::still_foreground(self.session, self.hwnd) {
            // The window may first repaint as active.
            std::thread::sleep(Duration::from_millis(150));
            return Ok(());
        }
        Err(host(format!(
            "the gallery lost the foreground to {thief}; no further input was sent"
        )))
    }

    /// The client area, in physical screen pixels.
    pub(super) fn client(&self) -> anyhow::Result<RECT> {
        self.owned()?;
        let mut info = WINDOWINFO {
            cbSize: u32::try_from(size_of::<WINDOWINFO>()).expect("BUG: WINDOWINFO fits u32"),
            ..WINDOWINFO::default()
        };
        // SAFETY: a sized out-parameter for the gallery's own live window.
        unsafe { GetWindowInfo(self.hwnd, &raw mut info) }?;
        Ok(info.rcClient)
    }

    /// Physical pixels per logical pixel.
    pub(super) fn scale(&self) -> anyhow::Result<f64> {
        // SAFETY: a pure DPI lookup for the gallery's window.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        anyhow::ensure!(dpi != 0, "the window's DPI is unavailable");
        Ok(f64::from(dpi) / 96.0)
    }

    /// `point` moved by a logical offset.
    pub(super) fn offset(&self, point: POINT, dx: f64, dy: f64) -> anyhow::Result<POINT> {
        let scale = self.scale()?;
        Ok(POINT {
            x: point.x + (dx * scale).round() as i32,
            y: point.y + (dy * scale).round() as i32,
        })
    }

    // ---------------------------------------------------------------------
    // Observations
    // ---------------------------------------------------------------------

    /// The one visible node of `kind` named `name` inside the client area,
    /// or `None` when there is none or more than one.
    pub(super) fn node(
        &self,
        kind: UIA_CONTROLTYPE_ID,
        name: &str,
    ) -> anyhow::Result<Option<IUIAutomationElement>> {
        self.find(kind, |candidate| candidate == name)
    }

    /// The one visible node of `kind` whose name satisfies `accept`.
    pub(super) fn find(
        &self,
        kind: UIA_CONTROLTYPE_ID,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<Option<IUIAutomationElement>> {
        let client = self.client()?;
        let mut found = None;
        for node in self.session.walk(&self.window) {
            if node.control != kind || !accept(&node.name) {
                continue;
            }
            // SAFETY: current properties on the session's COM thread.
            if unsafe { node.element.CurrentIsOffscreen()? }.as_bool() {
                continue;
            }
            let rect = uia::bounds(&node.element)?;
            if !overlaps(client, rect) {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            found = Some(node.element);
        }
        Ok(found)
    }

    /// Whether any node of `kind` named `name` is published at all.
    pub(super) fn published(&self, kind: UIA_CONTROLTYPE_ID, name: &str) -> bool {
        uia::has(&self.session.walk(&self.window), kind, name)
    }

    /// Waits for `observe` to yield a value.
    pub(super) fn wait<T>(
        &self,
        what: &str,
        mut observe: impl FnMut(&Self) -> anyhow::Result<Option<T>>,
    ) -> anyhow::Result<T> {
        let until = Instant::now() + WITHIN;
        loop {
            self.owned()?;
            if let Some(value) = observe(self)? {
                return Ok(value);
            }
            anyhow::ensure!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(POLL);
        }
    }

    /// Requires `holds` to stay true for a while.
    pub(super) fn stays(
        &self,
        what: &str,
        mut holds: impl FnMut(&Self) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        let until = Instant::now() + STILL;
        loop {
            anyhow::ensure!(holds(self)?, "{what} did not hold");
            if Instant::now() >= until {
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
    }

    pub(super) fn wait_node(
        &self,
        kind: UIA_CONTROLTYPE_ID,
        name: &str,
    ) -> anyhow::Result<IUIAutomationElement> {
        self.wait(&format!("{name:?} ({})", kind.0), |driver| {
            driver.node(kind, name)
        })
    }

    /// Waits for the readout, or any text, to read exactly `text`.
    pub(super) fn text(&self, text: &str) -> anyhow::Result<()> {
        self.wait_node(TEXT, text).map(drop)
    }

    /// Waits for a text starting with `prefix` that `accept` takes, and
    /// returns its whole name.
    pub(super) fn text_where(
        &self,
        prefix: &str,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        self.wait(&format!("a text {prefix:?}… as expected"), |driver| {
            Ok(driver
                .session
                .walk(&driver.window)
                .into_iter()
                .find(|node| {
                    node.control == TEXT && node.name.starts_with(prefix) && accept(&node.name)
                })
                .map(|node| node.name))
        })
    }

    /// Presses Tab until the focused node's name satisfies `accept`, at most
    /// `presses` times; returns its name.
    pub(super) fn tab_until(
        &self,
        presses: usize,
        accept: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        let mut seen = Vec::new();
        for _ in 0..presses {
            self.chord(&[VK_TAB])?;
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                let name = self.focus_name();
                if accept(&name) {
                    return Ok(name);
                }
                if Instant::now() >= deadline {
                    seen.push(name);
                    break;
                }
                std::thread::sleep(POLL);
            }
        }
        anyhow::bail!("Tab never reached the expected control; focus went {seen:?}")
    }

    /// Waits for no node of `kind` named `name` to be published.
    pub(super) fn gone(&self, kind: UIA_CONTROLTYPE_ID, name: &str) -> anyhow::Result<()> {
        self.wait(&format!("{name:?} to leave the tree"), |driver| {
            Ok((!driver.published(kind, name)).then_some(()))
        })
    }

    /// The bounds of the one visible node of `kind` named `name`.
    pub(super) fn bounds_of(&self, kind: UIA_CONTROLTYPE_ID, name: &str) -> anyhow::Result<RECT> {
        uia::bounds(&self.wait_node(kind, name)?)
    }

    /// Waits for the node of `kind` named `name` to have the keyboard focus.
    pub(super) fn focused(&self, kind: UIA_CONTROLTYPE_ID, name: &str) -> anyhow::Result<()> {
        self.wait(&format!("keyboard focus on {name:?}"), |driver| {
            Ok(driver
                .node(kind, name)?
                .filter(uia::has_keyboard_focus)
                .map(drop))
        })
    }

    /// The name of the node with the keyboard focus, for a failure message.
    pub(super) fn focus_name(&self) -> String {
        self.session
            .walk(&self.window)
            .iter()
            .find(|node| uia::has_keyboard_focus(&node.element))
            .map_or_else(|| "nothing".to_owned(), |node| node.name.clone())
    }

    /// The colour at `point`, after waiting for the screen to show
    /// `expected` there (within `tolerance`); fails with what it showed.
    pub(super) fn pixel(
        &self,
        what: &str,
        point: POINT,
        expected: Rgb,
        tolerance: u8,
    ) -> anyhow::Result<()> {
        let mut seen = None;
        let found = self.wait(what, |driver| {
            let capture = driver.capture()?;
            let color = capture
                .at(point)
                .ok_or_else(|| anyhow::anyhow!("{point:?} is outside the window"))?;
            seen = Some(color);
            Ok(color.near(expected, tolerance).then_some(()))
        });
        found.map_err(|_| {
            anyhow::anyhow!(
                "pixel {what} at ({}, {}) is {}, expected {expected}",
                point.x,
                point.y,
                seen.map_or_else(|| "unread".to_owned(), |color| color.to_string())
            )
        })
    }

    /// The colour at `point` now.
    pub(super) fn sample(&self, point: POINT) -> anyhow::Result<Rgb> {
        self.capture()?
            .at(point)
            .ok_or_else(|| anyhow::anyhow!("{point:?} is outside the window"))
    }

    /// The client area as the display shows it. The window is in front
    /// (checked), so the desktop's pixels there are its own.
    pub(super) fn capture(&self) -> anyhow::Result<Capture> {
        self.before_input()?;
        Capture::screen(self.client()?)
    }

    /// Saves the client area as `<name>.png` beside the run's other captures.
    pub(super) fn shoot(&self, name: &str) -> anyhow::Result<()> {
        let path = self.shots.join(format!("{name}.png"));
        self.capture()?.save_png(&path)?;
        println!("captured {}", path.display());
        Ok(())
    }

    /// The tree, with each named node's bounds, for a failure's diagnosis.
    pub(super) fn dump(&self) {
        let nodes = self.session.walk(&self.window);
        uia::dump(&nodes);
        for node in nodes.iter().filter(|node| !node.name.is_empty()) {
            if let Ok(rect) = uia::bounds(&node.element) {
                println!(
                    "  {:?} at ({}, {})-({}, {})",
                    node.name, rect.left, rect.top, rect.right, rect.bottom
                );
            }
        }
    }

    // ---------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------

    /// Opens a screen from the sidebar, freshly built, and waits for its
    /// title. A screen already showing is left first, so every check starts
    /// from the screen's initial state.
    fn open(&self, screen: &str) -> anyhow::Result<()> {
        if self.showing(screen) {
            let other = if screen == FIRST_SCREEN {
                "Focus and keys"
            } else {
                FIRST_SCREEN
            };
            self.navigate(other)?;
        }
        self.navigate(screen)?;
        // Let the screen's first frames land before it is driven.
        std::thread::sleep(Duration::from_millis(300));
        Ok(())
    }

    /// Clicks a sidebar entry and waits for its screen. Navigation is not
    /// what the checks measure: a click that changed nothing is reported and
    /// retried once, so the screen's own checks still run.
    fn navigate(&self, screen: &str) -> anyhow::Result<()> {
        let shown = |driver: &Self| Ok(driver.showing(screen).then_some(()));
        self.click_node(BUTTON, screen)?;
        if self.wait(&format!("the {screen:?} screen"), shown).is_ok() {
            return Ok(());
        }
        self.lost_clicks.set(self.lost_clicks.get() + 1);
        println!(
            "NOTE: a click on the {screen:?} sidebar entry changed nothing (focus on {:?}, foreground {}); clicking again",
            self.focus_name(),
            foreground_window()
        );
        self.click_node(BUTTON, screen)?;
        self.wait(&format!("the {screen:?} screen"), shown)
    }

    /// Whether `screen` is the one showing: its title is published beside
    /// the sidebar button of the same name.
    fn showing(&self, screen: &str) -> bool {
        self.session
            .walk(&self.window)
            .iter()
            .filter(|node| node.control == TEXT && node.name == screen)
            .count()
            >= 1
    }

    fn cursor_to(&self, point: POINT) -> anyhow::Result<()> {
        self.before_input()?;
        // SAFETY: moves the cursor; the hit check below decides whether any
        // press follows.
        unsafe { SetCursorPos(point.x, point.y) }
            .map_err(|error| host(format!("cursor move refused: {error}")))?;
        std::thread::sleep(HOLD);
        if !input::hits(self.session, self.hwnd, point) {
            return Err(host(format!(
                "({}, {}) does not hit the gallery",
                point.x, point.y
            )));
        }
        Ok(())
    }

    fn send(&self, events: &[INPUT]) -> anyhow::Result<()> {
        self.before_input()?;
        input::send(events).map_err(|error| host(format!("input refused: {error:#}")))
    }

    /// A left click at `point`.
    pub(super) fn click(&self, point: POINT) -> anyhow::Result<()> {
        self.cursor_to(point)?;
        let release = Release::new(vec![mouse(MOUSEEVENTF_LEFTUP, 0)]);
        self.send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0)])?;
        std::thread::sleep(HOLD);
        release.finish()?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// Two left clicks at `point`, inside the double-click time.
    pub(super) fn double_click(&self, point: POINT) -> anyhow::Result<()> {
        self.cursor_to(point)?;
        for _ in 0..2 {
            let release = Release::new(vec![mouse(MOUSEEVENTF_LEFTUP, 0)]);
            self.send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0)])?;
            std::thread::sleep(Duration::from_millis(30));
            release.finish()?;
            std::thread::sleep(Duration::from_millis(60));
        }
        Ok(())
    }

    /// A left press at `point` held for `hold`.
    pub(super) fn press_and_hold(&self, point: POINT, hold: Duration) -> anyhow::Result<()> {
        self.cursor_to(point)?;
        let release = Release::new(vec![mouse(MOUSEEVENTF_LEFTUP, 0)]);
        self.send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0)])?;
        std::thread::sleep(hold);
        release.finish()?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// A right click at `point`.
    pub(super) fn right_click(&self, point: POINT) -> anyhow::Result<()> {
        self.cursor_to(point)?;
        let release = Release::new(vec![mouse(MOUSEEVENTF_RIGHTUP, 0)]);
        self.send(&[mouse(MOUSEEVENTF_RIGHTDOWN, 0)])?;
        std::thread::sleep(HOLD);
        release.finish()?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// A click at the centre of the one visible node of `kind` named `name`.
    pub(super) fn click_node(&self, kind: UIA_CONTROLTYPE_ID, name: &str) -> anyhow::Result<()> {
        let rect = self.bounds_of(kind, name)?;
        self.click(input::centre(rect))
    }

    /// A left-button mouse drag from `from` to `to` in `steps` moves, held
    /// still at the end for `settle` before the release so no fling follows
    /// when `settle` is long.
    pub(super) fn mouse_drag(
        &self,
        from: POINT,
        to: POINT,
        steps: u32,
        settle: Duration,
    ) -> anyhow::Result<()> {
        self.cursor_to(from)?;
        let release = Release::new(vec![mouse(MOUSEEVENTF_LEFTUP, 0)]);
        self.send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0)])?;
        std::thread::sleep(HOLD);
        for step in 1..=steps {
            let t = f64::from(step) / f64::from(steps);
            let at = POINT {
                x: from.x + (f64::from(to.x - from.x) * t).round() as i32,
                y: from.y + (f64::from(to.y - from.y) * t).round() as i32,
            };
            self.before_input()?;
            // SAFETY: moves the cursor while the gallery holds the capture.
            unsafe { SetCursorPos(at.x, at.y) }
                .map_err(|error| host(format!("cursor move refused: {error}")))?;
            std::thread::sleep(Duration::from_millis(16));
        }
        std::thread::sleep(settle);
        release.finish()?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// `notches` wheel notches at `point`, positive away from the user
    /// (scrolling up), with Control held when `control`.
    pub(super) fn wheel(&self, point: POINT, notches: i32, control: bool) -> anyhow::Result<()> {
        self.cursor_to(point)?;
        let held = control.then(|| Release::new(vec![key(VK_CONTROL, KEYEVENTF_KEYUP)]));
        if control {
            self.send(&[key(VK_CONTROL, KEYBD_EVENT_FLAGS(0))])?;
            std::thread::sleep(HOLD);
        }
        for _ in 0..notches.unsigned_abs() {
            let delta = if notches > 0 { 120 } else { -120 };
            self.send(&[mouse(MOUSEEVENTF_WHEEL, delta)])?;
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Some(held) = held {
            held.finish()?;
        }
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// Presses the keys of `chord` in order and releases them in reverse.
    pub(super) fn chord(&self, chord: &[VIRTUAL_KEY]) -> anyhow::Result<()> {
        let release = Release::new(
            chord
                .iter()
                .rev()
                .map(|&vk| key(vk, KEYEVENTF_KEYUP))
                .collect(),
        );
        for &vk in chord {
            self.send(&[key(vk, KEYBD_EVENT_FLAGS(0))])?;
            std::thread::sleep(HOLD);
        }
        release.finish()?;
        std::thread::sleep(Duration::from_millis(80));
        Ok(())
    }

    /// Types `text` as Unicode characters, as an on-screen keyboard or an
    /// input method delivers them.
    pub(super) fn type_text(&self, text: &str) -> anyhow::Result<()> {
        for unit in text.encode_utf16() {
            let packet = |flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: unit,
                        dwFlags: flags,
                        ..KEYBDINPUT::default()
                    },
                },
            };
            let release = Release::new(vec![packet(KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]);
            self.send(&[packet(KEYEVENTF_UNICODE)])?;
            std::thread::sleep(HOLD);
            release.finish()?;
        }
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// The touchscreen, or why touch cannot be verified on this host.
    fn touchscreen(&self) -> anyhow::Result<&Touchscreen> {
        self.touch
            .as_ref()
            .ok_or_else(|| host("this host refuses synthetic touch"))
    }

    fn touch_ready(&self, points: &[POINT]) -> anyhow::Result<()> {
        self.before_input()?;
        for &point in points {
            if !input::hits(self.session, self.hwnd, point) {
                return Err(host(format!(
                    "touch at ({}, {}) would miss the gallery",
                    point.x, point.y
                )));
            }
        }
        Ok(())
    }

    /// One finger from `from` to `to` over `duration`, then held still for
    /// `hold` before it lifts.
    pub(super) fn touch_drag(
        &self,
        from: POINT,
        to: POINT,
        duration: Duration,
        hold: Duration,
    ) -> anyhow::Result<()> {
        self.touch_ready(&[from, to])?;
        let steps = (duration.as_millis() / 16).max(1) as u32;
        self.touchscreen()?
            .stroke(&[(from, to)], steps, Duration::from_millis(16), hold)
            .map_err(|error| host(format!("touch injection refused: {error}")))?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    /// A finger tap at `point`.
    pub(super) fn touch_tap(&self, point: POINT) -> anyhow::Result<()> {
        self.touch_ready(&[point])?;
        self.touchscreen()?
            .tap(point, Duration::from_millis(40))
            .map_err(|error| host(format!("touch injection refused: {error}")))?;
        Ok(())
    }

    /// Two fingers from `a.0`/`b.0` to `a.1`/`b.1`: a pinch or a spread.
    pub(super) fn touch_pinch(
        &self,
        a: (POINT, POINT),
        b: (POINT, POINT),
        duration: Duration,
    ) -> anyhow::Result<()> {
        self.touch_ready(&[a.0, a.1, b.0, b.1])?;
        let steps = (duration.as_millis() / 16).max(1) as u32;
        self.touchscreen()?
            .stroke(
                &[a, b],
                steps,
                Duration::from_millis(16),
                Duration::from_millis(80),
            )
            .map_err(|error| host(format!("touch injection refused: {error}")))?;
        std::thread::sleep(HOLD);
        Ok(())
    }
}

/// Asks Windows to bring `hwnd` forward and waits briefly for it.
fn acquire_foreground(hwnd: HWND) -> bool {
    // SAFETY: a foreground request for the gallery's own window, then reads.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        // SAFETY: reads foreground state only.
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// The foreground window's class and title, to name what took the
/// foreground.
fn foreground_window() -> String {
    let mut class = [0_u16; 128];
    let mut title = [0_u16; 256];
    // SAFETY: reads the foreground window's class and title into local
    // buffers whose lengths the calls receive.
    let (class_len, title_len) = unsafe {
        let window = GetForegroundWindow();
        (
            GetClassNameW(window, &mut class),
            GetWindowTextW(window, &mut title),
        )
    };
    let text = |buffer: &[u16], len: i32| {
        String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0).min(buffer.len())])
    };
    format!(
        "a window of class {:?} titled {:?}",
        text(&class, class_len),
        text(&title, title_len)
    )
}

fn overlaps(client: RECT, rect: RECT) -> bool {
    rect.right > rect.left
        && rect.bottom > rect.top
        && rect.left < client.right
        && rect.right > client.left
        && rect.top < client.bottom
        && rect.bottom > client.top
}

fn mouse(
    flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
    wheel: i32,
) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                mouseData: wheel.cast_unsigned(),
                ..MOUSEINPUT::default()
            },
        },
    }
}

/// A key event as a keyboard sends it, with the extended flag on the keys
/// a real keyboard sends from its extended block (arrows, Home, the menu
/// key), so they are not read as the numeric keypad's.
fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    let mut event = input::keyboard(vk, flags);
    let extended = [
        VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_INSERT,
        VK_DELETE, VK_APPS,
    ];
    if extended.contains(&vk) {
        // SAFETY: `keyboard` built the keyboard arm of the union.
        unsafe { event.Anonymous.ki.dwFlags |= KEYEVENTF_EXTENDEDKEY };
    }
    event
}

/// Releases a press that already went out, on every path.
struct Release(Option<Vec<INPUT>>);

impl Release {
    fn new(inputs: Vec<INPUT>) -> Self {
        Self(Some(inputs))
    }

    fn finish(mut self) -> anyhow::Result<()> {
        let inputs = self.0.take().expect("BUG: a release is finished once");
        input::release(&inputs).map_err(|error| host(format!("input release refused: {error:#}")))
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        if let Some(inputs) = self.0.take()
            && let Err(error) = input::release(&inputs)
        {
            eprintln!("CANNOT_VERIFY: an outstanding release failed: {error:#}");
        }
    }
}
