//! The existing Notes production tree driven on its real Windows window.
//!
//! UIA reads current roles, names, values and bounds and invokes normal buttons.
//! Pointer events and physical shortcuts enter the OS input queue. Character
//! entry uses UTF-16 Unicode packets (VK_PACKET -> TranslateMessage -> WM_CHAR),
//! not a controller setter. This is plain native keyboard input, not a TSF/IME
//! composition or Narrator test. UIA observations are not GPU pixel readbacks.
//! The deterministic Error/Retry flow does not prove held-loader cancellation;
//! that remains the separate, already executed public headless scenario.

use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Accessibility::{
    IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationValuePattern,
    UIA_CONTROLTYPE_ID, UIA_EditControlTypeId, UIA_GroupControlTypeId, UIA_InvokePatternId,
    UIA_ValuePatternId,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_F4, VK_MENU, VK_RETURN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowInfo, GetWindowThreadProcessId, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    SetCursorPos, SetWindowPos, WINDOWINFO,
};

use super::uia::{self, BUTTON, Session, Start, TEXT};
use super::windows_input::{self as input, Cursor};

const WITHIN: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(100);
const HOLD: Duration = Duration::from_millis(40);
const STILL: Duration = Duration::from_millis(400);
const FIRST: &str = "native first";
const DRAFT: &str = "native retained draft";
const RESIZED_DRAFT: &str = "native resized draft";

#[derive(Debug)]
struct CannotVerify(String);

impl fmt::Display for CannotVerify {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CannotVerify {}

fn host_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CannotVerify(message.into()))
}

/// Exit 0 only after observed graceful process exit; 1 failure, 2 host refusal.
pub(super) fn run(probe: &Path) -> anyhow::Result<u8> {
    // SAFETY: process-local setup before this driver creates any window.
    if let Err(error) =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
    {
        println!("NOTES=CANNOT_VERIFY: DPI awareness refused: {error}");
        return Ok(2);
    }
    let mut session = match Session::start(probe) {
        Ok(session) => session,
        Err(Start::CannotVerify(why)) => {
            println!("NOTES=CANNOT_VERIFY: {why}");
            return Ok(2);
        }
        Err(Start::Failed(error)) => return Err(error),
    };
    let cursor = Cursor::save();
    let result = drive(&mut session);
    // Reuse the existing cursor and process cleanup guards on every exit. A
    // forced Probe Drop is fallback cleanup and can never establish close PASS.
    drop(cursor);
    drop(session);
    match result {
        Ok(()) => {
            println!("NOTES=PASS");
            Ok(0)
        }
        Err(error) if error.downcast_ref::<CannotVerify>().is_some() => {
            println!("NOTES=CANNOT_VERIFY: {error:#}");
            Ok(2)
        }
        Err(error) => {
            println!("NOTES=FAIL: {error:#}");
            Ok(1)
        }
    }
}

fn drive(session: &mut Session) -> anyhow::Result<()> {
    let window = session
        .window_with_button("Retry")?
        .ok_or_else(|| anyhow::anyhow!("initial Notes error never published Retry"))?;
    // SAFETY: current properties on the session's COM thread, not cached data.
    let (hwnd, pid) = unsafe {
        (
            window.CurrentNativeWindowHandle()?,
            window.CurrentProcessId()?,
        )
    };
    let pid = u32::try_from(pid)?;
    let mut driver = Driver {
        session,
        window,
        hwnd,
        pid,
    };
    let result = driver.flow();
    if result.is_err() {
        println!("last native Notes tree:");
        driver.dump();
    }
    result
}

struct Driver<'a> {
    session: &'a mut Session,
    window: IUIAutomationElement,
    hwnd: HWND,
    pid: u32,
}

impl Driver<'_> {
    fn flow(&mut self) -> anyhow::Result<()> {
        self.owned()?;
        if input::foreground(self.hwnd).is_none() {
            return Err(host_error("Windows refused Notes foreground ownership"));
        }
        self.resize(640, 720)?;
        self.text("Load failed")?;
        let error_value = self.wait_node(BUTTON, Some("Retry"))?;
        let retry = uia::bounds(&error_value)?;
        let client = self.client()?;
        // A real missed click is the hit-test control, not an accessibility action.
        let miss = POINT {
            x: client.right - 12,
            y: client.bottom - 12,
        };
        anyhow::ensure!(!contains(retry, miss), "miss point overlaps Retry");
        self.click(miss)?;
        self.unchanged("missed Retry click", |driver| {
            Ok(driver.node(TEXT, Some("Load failed"))?.is_some()
                && driver.node(BUTTON, Some("Retry"))?.is_some())
        })?;
        self.click_button("Retry")?;
        self.wait_node(BUTTON, Some("Note 0"))?;
        self.spacing("Note 0", "Note 1", 48.0)?;
        println!("NOTES_STAGE=error_retry_home");

        self.open("Note 0", 0, "Note 0")?;
        self.replace("")?;
        self.click_button("Save note")?;
        self.editor(0, true)?;
        self.value("")?;
        self.text("Fix the title")?;
        self.invoke("Back")?;
        self.wait_node(BUTTON, Some("Note 0"))?;
        anyhow::ensure!(
            self.node(BUTTON, Some(FIRST))?.is_none(),
            "invalid Save changed Home"
        );
        println!("NOTES_STAGE=invalid_save_preserves_home");

        self.open("Note 0", 0, "")?;
        self.replace(FIRST)?;
        self.save_by_keyboard()?;
        self.invoke("Back")?;
        self.home_title(FIRST)?;
        println!("NOTES_STAGE=keyboard_save_committed");

        self.open(FIRST, 0, FIRST)?;
        self.replace(DRAFT)?;
        self.invoke("Settings")?;
        self.text("Compact rows: false")?;
        self.invoke("Toggle compact rows")?;
        self.text("Compact rows: true")?;
        self.invoke("Back")?;
        self.editor(0, false)?;
        self.value(DRAFT)?;
        // Back to Home proves the unsaved draft did not modify the saved row.
        self.invoke("Back")?;
        self.home_title(FIRST)?;
        self.spacing("Note 1", "Note 2", 32.0)?;
        self.open(FIRST, 0, DRAFT)?;
        self.click_button("Save note")?;
        self.text("Saved note 0")?;
        self.invoke("Back")?;
        self.home_title(DRAFT)?;
        anyhow::ensure!(
            self.node(BUTTON, Some(FIRST))?.is_none(),
            "old saved row still visible"
        );
        println!("NOTES_STAGE=retained_draft_compact_saved_home");

        self.scroll_and_retain()?;
        self.resize(720, 640)?;
        self.wait_node(BUTTON, Some("Reload notes"))?;
        self.text("Saved note 0")?;
        let rows = self.stable_band()?;
        let row = rows
            .first()
            .ok_or_else(|| anyhow::anyhow!("resize lost all visible rows"))?;
        let id = row.id;
        self.open(&format!("Note {id}"), id, &format!("Note {id}"))?;
        self.replace(RESIZED_DRAFT)?;
        self.resize(640, 720)?;
        self.value(RESIZED_DRAFT)?;
        self.invoke("Back")?;
        self.stable_band()?;
        self.open(&format!("Note {id}"), id, RESIZED_DRAFT)?;
        println!("NOTES_STAGE=resize_routes_usable");

        self.chord(&[VK_MENU, VK_F4])?;
        let status = self
            .session
            .wait_for_exit(Duration::from_secs(15))?
            .ok_or_else(|| anyhow::anyhow!("Alt+F4 did not produce graceful Notes process exit"))?;
        anyhow::ensure!(status.success(), "Notes exited unsuccessfully: {status}");
        println!("NOTES_STAGE=graceful_close status={status}");
        Ok(())
    }

    fn owned(&self) -> anyhow::Result<()> {
        let mut pid = 0;
        // SAFETY: read-only native identity lookup; original PID never changes.
        let thread = unsafe { GetWindowThreadProcessId(self.hwnd, Some(&raw mut pid)) };
        anyhow::ensure!(
            thread != 0 && pid == self.pid,
            "Notes HWND no longer belongs to original process"
        );
        Ok(())
    }

    fn before_input(&self) -> anyhow::Result<()> {
        self.owned()?;
        if !input::still_foreground(self.hwnd) {
            return Err(host_error(
                "Notes lost the foreground; no further input sent",
            ));
        }
        Ok(())
    }

    fn client(&self) -> anyhow::Result<RECT> {
        Ok(self.window_info()?.rcClient)
    }

    fn window_info(&self) -> anyhow::Result<WINDOWINFO> {
        self.owned()?;
        let mut info = WINDOWINFO {
            cbSize: u32::try_from(size_of::<WINDOWINFO>()).expect("BUG: WINDOWINFO fits u32"),
            ..WINDOWINFO::default()
        };
        // SAFETY: initialized, correctly sized output for this owned HWND.
        // rcClient is already in physical screen coordinates; no GDI dependency.
        unsafe { GetWindowInfo(self.hwnd, &raw mut info) }?;
        Ok(info)
    }

    fn dpi(&self) -> anyhow::Result<f64> {
        self.owned()?;
        // SAFETY: pure DPI lookup for the owned native window.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        anyhow::ensure!(dpi != 0, "Notes DPI is unavailable");
        Ok(f64::from(dpi) / 96.0)
    }

    fn resize(&self, width: i32, height: i32) -> anyhow::Result<()> {
        self.before_input()?;
        anyhow::ensure!(width > 0 && height > 0, "resize must remain positive");
        let scale = self.dpi()?;
        // SAFETY: read-only DPI lookup; integer conversion preserves exact
        // rounded native dimensions without float-to-integer truncation.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        anyhow::ensure!(dpi != 0, "resize DPI unavailable");
        let info = self.window_info()?;
        let outer = info.rcWindow;
        let client = info.rcClient;
        let physical_width = i32::try_from((i64::from(width) * i64::from(dpi) + 48) / 96)?;
        let physical_height = i32::try_from((i64::from(height) * i64::from(dpi) + 48) / 96)?;
        let full_width = i32::try_from(
            i64::from(physical_width) + i64::from(outer.right)
                - i64::from(outer.left)
                - (i64::from(client.right) - i64::from(client.left)),
        )?;
        let full_height = i32::try_from(
            i64::from(physical_height) + i64::from(outer.bottom)
                - i64::from(outer.top)
                - (i64::from(client.bottom) - i64::from(client.top)),
        )?;
        // SAFETY: resize only this process-owned live HWND; preserve z-order,
        // position and activation. This is an actual native resize, not layout injection.
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                full_width,
                full_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| host_error(format!("native resize refused: {error}")))?;
        self.wait("native client resize", |driver| {
            let rect = driver.client()?;
            Ok(((rect.right - rect.left - physical_width).abs() <= 1
                && (rect.bottom - rect.top - physical_height).abs() <= 1)
                .then_some(()))
        })?;
        println!("NOTES_RESIZE logical={width}x{height} scale={scale}");
        Ok(())
    }

    fn node(
        &self,
        kind: UIA_CONTROLTYPE_ID,
        name: Option<&str>,
    ) -> anyhow::Result<Option<IUIAutomationElement>> {
        let client = self.client()?;
        let mut selected = None;
        for node in self.session.walk(&self.window) {
            if node.control != kind || name.is_some_and(|name| name != node.name) {
                continue;
            }
            // SAFETY: read current properties on this session's COM thread.
            let visible = unsafe { !node.element.CurrentIsOffscreen()?.as_bool() };
            let bounds = uia::bounds(&node.element)?;
            if !visible || !inside(client, bounds) {
                continue;
            }
            if (kind == BUTTON || kind == UIA_EditControlTypeId)
                // SAFETY: current enabled state on this session's COM thread.
                && !unsafe { node.element.CurrentIsEnabled()? }.as_bool()
            {
                continue;
            }
            if selected.is_some() {
                return Ok(None);
            } // Never act on an ambiguous page.
            selected = Some(node.element);
        }
        Ok(selected)
    }

    fn wait<T>(
        &self,
        label: &str,
        mut observe: impl FnMut(&Self) -> anyhow::Result<Option<T>>,
    ) -> anyhow::Result<T> {
        let until = Instant::now() + WITHIN;
        loop {
            self.owned()?;
            if let Some(value) = observe(self)? {
                return Ok(value);
            }
            if Instant::now() >= until {
                anyhow::bail!("native Notes timed out: {label}");
            }
            std::thread::sleep(POLL);
        }
    }

    fn wait_node(
        &self,
        kind: UIA_CONTROLTYPE_ID,
        name: Option<&str>,
    ) -> anyhow::Result<IUIAutomationElement> {
        self.wait(
            &format!("unique visible node {name:?} ({})", kind.0),
            |driver| driver.node(kind, name),
        )
    }

    fn text(&self, name: &str) -> anyhow::Result<()> {
        self.wait_node(TEXT, Some(name))?;
        Ok(())
    }

    fn value(&self, expected: &str) -> anyhow::Result<IUIAutomationElement> {
        self.wait(&format!("committed editor value {expected:?}"), |driver| {
            let Some(field) = driver.node(UIA_EditControlTypeId, None)? else {
                return Ok(None);
            };
            // SAFETY: read-only ValuePattern query. Never call SetValue.
            let value = unsafe {
                field
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)?
                    .CurrentValue()?
            }
            .to_string();
            Ok((value == expected).then_some(field))
        })
    }

    fn click(&self, point: POINT) -> anyhow::Result<()> {
        self.before_input()?;
        let guard = Release::new(vec![input::mouse(MOUSEEVENTF_LEFTUP)]);
        let sent = input::click(self.hwnd, point)
            .map_err(|error| host_error(format!("native click refused: {error:#}")))?;
        if !sent {
            return Err(host_error(
                "click could not reach owned foreground Notes window",
            ));
        }
        // The delegated click completed its release. Errors keep our retry armed.
        guard.disarm();
        Ok(())
    }

    fn click_button(&self, name: &str) -> anyhow::Result<()> {
        let element = self.wait_node(BUTTON, Some(name))?;
        // SAFETY: current enabled property on the session's COM thread.
        anyhow::ensure!(
            unsafe { element.CurrentIsEnabled()? }.as_bool(),
            "button {name:?} disabled"
        );
        self.click(input::centre(uia::bounds(&element)?))
    }

    fn invoke(&self, name: &str) -> anyhow::Result<()> {
        let element = self.wait_node(BUTTON, Some(name))?;
        self.before_input()?;
        // SAFETY: invokes the normal currently published button action. It
        // does not bypass the application callback, router or validation.
        unsafe {
            element
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?
                .Invoke()
        }?;
        Ok(())
    }

    fn chord(&self, keys: &[VIRTUAL_KEY]) -> anyhow::Result<()> {
        self.before_input()?;
        let releases = keys
            .iter()
            .rev()
            .map(|key| input::keyboard(*key, KEYEVENTF_KEYUP))
            .collect();
        let guard = Release::new(releases);
        for key in keys {
            self.before_input()?;
            input::send(&[input::keyboard(*key, KEYBD_EVENT_FLAGS(0))]).map_err(|error| {
                host_error(format!("physical key injection refused: {error:#}"))
            })?;
            std::thread::sleep(HOLD);
        }
        guard.finish()?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    fn character(&self, unit: u16) -> anyhow::Result<()> {
        self.before_input()?;
        let packet = |up| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: if up {
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                    } else {
                        KEYEVENTF_UNICODE
                    },
                    ..KEYBDINPUT::default()
                },
            },
        };
        let guard = Release::new(vec![packet(true)]);
        input::send(&[packet(false)])
            .map_err(|error| host_error(format!("Unicode packet refused: {error:#}")))?;
        std::thread::sleep(HOLD);
        guard.finish()?;
        std::thread::sleep(HOLD);
        Ok(())
    }

    fn focus_edit(&self) -> anyhow::Result<()> {
        let field = self.wait_node(UIA_EditControlTypeId, None)?;
        self.click(input::centre(uia::bounds(&field)?))?;
        self.wait("actual editor keyboard focus", |driver| {
            Ok(driver
                .node(UIA_EditControlTypeId, None)?
                .filter(uia::has_keyboard_focus)
                .map(|_| ()))
        })
    }

    fn replace(&self, text: &str) -> anyhow::Result<()> {
        self.focus_edit()?;
        self.chord(&[VK_CONTROL, VIRTUAL_KEY(0x41)])?;
        self.chord(&[VK_BACK])?;
        self.value("")?; // Proves full Ctrl+A replacement, not mere key admission.
        for unit in text.encode_utf16() {
            self.character(unit)?;
        }
        self.value(text)?;
        println!("NOTES_VALUE={text:?}");
        Ok(())
    }

    fn focused(&self, kind: UIA_CONTROLTYPE_ID, name: Option<&str>) -> anyhow::Result<()> {
        self.wait(&format!("keyboard focus {name:?}"), |driver| {
            Ok(driver
                .node(kind, name)?
                .filter(uia::has_keyboard_focus)
                .map(|_| ()))
        })
    }

    fn save_by_keyboard(&self) -> anyhow::Result<()> {
        self.chord(&[VK_TAB])?;
        self.focused(BUTTON, Some("Save note"))?;
        self.chord(&[VK_SHIFT, VK_TAB])?;
        self.focused(UIA_EditControlTypeId, None)?;
        self.chord(&[VK_TAB])?;
        self.focused(BUTTON, Some("Save note"))?;
        self.chord(&[VK_RETURN])?;
        self.text("Saved note 0")?;
        self.editor(0, false)?;
        Ok(())
    }

    fn open(&self, title: &str, id: usize, expected: &str) -> anyhow::Result<()> {
        self.click_button(title)?;
        self.editor(id, false)?;
        self.value(expected)?;
        Ok(())
    }

    fn editor(&self, id: usize, invalid: bool) -> anyhow::Result<()> {
        // Form is a semantics boundary. Its passive heading and decoration
        // labels absorb in traversal order; Edit and Save remain child nodes.
        // Require the complete current name, including validation when present.
        let suffix = if invalid { " Enter a title" } else { "" };
        let name = format!("Editing note {id} Title{suffix}");
        let group = self.wait_node(UIA_GroupControlTypeId, Some(&name))?;
        if !invalid {
            anyhow::ensure!(
                self.node(
                    UIA_GroupControlTypeId,
                    Some(&format!("Editing note {id} Title Enter a title")),
                )?
                .is_none(),
                "normal Notes form retained a validation-error group"
            );
        }
        let nodes = self.session.walk(&group);
        anyhow::ensure!(
            nodes
                .iter()
                .filter(|node| node.control == UIA_EditControlTypeId)
                .count()
                == 1
                && nodes
                    .iter()
                    .filter(|node| node.control == BUTTON && node.name == "Save note")
                    .count()
                    == 1,
            "named Notes form must contain its sole Edit and Save button"
        );
        Ok(())
    }

    fn home_title(&self, title: &str) -> anyhow::Result<()> {
        self.wait_node(BUTTON, Some(title))?;
        self.wait_node(BUTTON, Some("Note 1"))?;
        anyhow::ensure!(
            self.node(BUTTON, Some("Note 0"))?.is_none(),
            "old title still in visible Home"
        );
        Ok(())
    }

    fn spacing(&self, first: &str, second: &str, logical: f64) -> anyhow::Result<()> {
        self.wait("actual row density", |driver| {
            let Some(first) = driver.node(BUTTON, Some(first))? else {
                return Ok(None);
            };
            let Some(second) = driver.node(BUTTON, Some(second))? else {
                return Ok(None);
            };
            let gap = uia::bounds(&second)?.top - uia::bounds(&first)?.top;
            let measured = f64::from(gap) / driver.dpi()?;
            Ok(((measured - logical).abs() <= 1.0 / driver.dpi()?).then_some(measured))
        })?;
        println!("NOTES_ROW_SPACING={logical}");
        Ok(())
    }

    fn unchanged(
        &self,
        label: &str,
        mut predicate: impl FnMut(&Self) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        let until = Instant::now() + STILL;
        loop {
            anyhow::ensure!(predicate(self)?, "native Notes changed during {label}");
            if Instant::now() >= until {
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
    }

    fn band(&self) -> anyhow::Result<Vec<Row>> {
        let mut client = self.client()?;
        for title in ["Settings", "Back", "Reload notes"] {
            if let Some(button) = self.node(BUTTON, Some(title))? {
                client.top = client.top.max(uia::bounds(&button)?.bottom);
            }
        }
        if let Some(status) = self.node(TEXT, Some("Saved note 0"))? {
            client.top = client.top.max(uia::bounds(&status)?.bottom);
        }
        let mut rows = Vec::new();
        for node in self.session.walk(&self.window) {
            if node.control != BUTTON {
                continue;
            }
            let Some(id) = node
                .name
                .strip_prefix("Note ")
                .and_then(|id| id.parse::<usize>().ok())
            else {
                continue;
            };
            // SAFETY: current offscreen property on the session's COM thread.
            if unsafe { node.element.CurrentIsOffscreen()? }.as_bool() {
                continue;
            }
            let rect = uia::bounds(&node.element)?;
            if inside(client, rect) {
                rows.push(Row { id, rect });
            }
        }
        rows.sort_by_key(|row| row.id);
        anyhow::ensure!(
            !rows.windows(2).any(|rows| rows[0].id == rows[1].id),
            "ambiguous retained Home band"
        );
        Ok(rows)
    }

    fn stable_band(&self) -> anyhow::Result<Vec<Row>> {
        let mut previous = Vec::new();
        let mut stable_since = Instant::now();
        self.wait("stable visible lazy band", |driver| {
            let rows = driver.band()?;
            if rows.is_empty() || rows != previous {
                previous.clone_from(&rows);
                stable_since = Instant::now();
                return Ok(None);
            }
            Ok((stable_since.elapsed() >= STILL).then_some(rows))
        })
    }

    fn move_pointer(&self, point: POINT) -> anyhow::Result<()> {
        self.before_input()?;
        // SAFETY: physical cursor movement, followed by foreground/hit checks.
        unsafe { SetCursorPos(point.x, point.y) }
            .map_err(|error| host_error(format!("cursor move refused: {error}")))?;
        std::thread::sleep(HOLD);
        self.before_input()?;
        if !input::hits(self.hwnd, point) {
            return Err(host_error("drag/wheel point does not hit Notes"));
        }
        Ok(())
    }

    fn scroll_and_retain(&self) -> anyhow::Result<()> {
        let before = self.stable_band()?;
        anyhow::ensure!(
            before.len() >= 6,
            "not enough visible rows for bounded drag"
        );
        let first = before.first().expect("BUG: six rows checked");
        let last = before.last().expect("BUG: six rows checked");
        let start = input::centre(last.rect);
        let end = POINT {
            x: start.x,
            y: first.rect.bottom + 8,
        };
        self.move_pointer(start)?;
        let guard = Release::new(vec![input::mouse(MOUSEEVENTF_LEFTUP)]);
        input::send(&[input::mouse(MOUSEEVENTF_LEFTDOWN)])
            .map_err(|error| host_error(format!("drag press refused: {error:#}")))?;
        std::thread::sleep(HOLD);
        self.move_pointer(POINT {
            x: start.x,
            y: start.y - 20,
        })?;
        self.move_pointer(end)?;
        // Expire the velocity sample before Up: retained-offset acceptance
        // must not depend on ballistic progress racing route transitions.
        std::thread::sleep(STILL);
        guard.finish()?;
        let dragged = self.stable_band()?;
        anyhow::ensure!(
            dragged.first().is_some_and(|row| row.id > first.id),
            "native drag did not move lazy band"
        );

        let wheel_point = input::centre(dragged[dragged.len() / 2].rect);
        self.move_pointer(wheel_point)?;
        let wheel = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: MOUSEEVENTF_WHEEL,
                    mouseData: u32::from_ne_bytes((-120_i32).to_ne_bytes()),
                    ..MOUSEINPUT::default()
                },
            },
        };
        self.before_input()?;
        input::send(&[wheel])
            .map_err(|error| host_error(format!("wheel injection refused: {error:#}")))?;
        self.wait("wheel changes actual row position", |driver| {
            let rows = driver.band()?;
            Ok((!rows.is_empty() && rows != dragged).then_some(()))
        })?;
        let settled = self.stable_band()?;
        let target = settled
            .first()
            .ok_or_else(|| anyhow::anyhow!("wheel lost visible band"))?;
        self.open(
            &format!("Note {}", target.id),
            target.id,
            &format!("Note {}", target.id),
        )?;
        self.invoke("Back")?;
        let returned = self.stable_band()?;
        let row = returned
            .iter()
            .find(|row| row.id == target.id)
            .ok_or_else(|| anyhow::anyhow!("navigation lost retained visible row {}", target.id))?;
        anyhow::ensure!(
            returned.first().map(|row| row.id) == settled.first().map(|row| row.id)
                && (row.rect.top - target.rect.top).abs() <= 1,
            "navigation changed retained band/row position"
        );
        println!("NOTES_STAGE=drag_wheel_retained_band first={}", target.id);
        // This measures native band residency/position, not eager construction
        // counts, a memory budget or a frame-time benchmark.
        Ok(())
    }

    fn dump(&self) {
        let nodes = self.session.walk(&self.window);
        uia::dump(&nodes);
        for node in nodes {
            if let Ok(rect) = uia::bounds(&node.element) {
                println!("NOTES_NODE name={:?} bounds={rect:?}", node.name);
            }
            if node.control == UIA_EditControlTypeId {
                // SAFETY: best-effort read-only diagnostic after the first
                // failure. It cannot turn a failed observation into success.
                if let Ok(value) = unsafe {
                    node.element
                        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        .and_then(|pattern| pattern.CurrentValue())
                } {
                    println!("NOTES_EDITOR_VALUE={:?}", value.to_string());
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: usize,
    rect: RECT,
}

fn inside(client: RECT, rect: RECT) -> bool {
    rect.right > rect.left
        && rect.bottom > rect.top
        && rect.left >= client.left
        && rect.right <= client.right
        && rect.top >= client.top
        && rect.bottom <= client.bottom
}

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

/// Every possibly injected press owns its releases before sending any Down.
/// Normal and early-error paths reuse the existing retrying release routine.
struct Release(Option<Vec<INPUT>>);
impl Release {
    fn new(inputs: Vec<INPUT>) -> Self {
        Self(Some(inputs))
    }
    fn disarm(mut self) {
        self.0 = None;
    }
    fn finish(mut self) -> anyhow::Result<()> {
        let inputs = self.0.as_ref().expect("BUG: release guard remains armed");
        input::release(inputs)
            .map_err(|error| host_error(format!("input release refused: {error:#}")))?;
        self.0 = None;
        Ok(())
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(inputs) = self.0.take()
            && let Err(error) = input::release(&inputs)
        {
            eprintln!("CANNOT_VERIFY: outstanding input release failed: {error:#}");
        }
    }
}
