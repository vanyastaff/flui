//! A UI Automation client over one launched probe, shared by the Windows
//! checks: what Narrator sees of a window, read through the same API.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HANDLE, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetUserObjectInformationW,
    OpenInputDesktop, UOI_NAME,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
    TreeScope_Children, UIA_CONTROLTYPE_ID, UIA_ProcessIdPropertyId,
};

pub(super) use windows::Win32::UI::Accessibility::{
    UIA_ButtonControlTypeId as BUTTON, UIA_TextControlTypeId as TEXT,
};

/// How long the probe gets to put its window up and publish a first tree.
/// The first query is also what activates the adapter, so the tree can trail
/// the window by a frame.
pub(super) const APPEAR_WITHIN: Duration = Duration::from_secs(15);
/// The variable `examples/a11y_probe.rs` reads for how many whole seconds it
/// stays up before quitting on its own.
pub(super) const RUN_FOR_ENV: &str = "FLUI_PROBE_RUN_FOR_SECS";
const POLL: Duration = Duration::from_millis(100);

/// One node of the raw view, as an assistive technology reads it.
pub(super) struct Node {
    pub(super) element: IUIAutomationElement,
    pub(super) control: UIA_CONTROLTYPE_ID,
    pub(super) name: String,
    depth: usize,
}

/// COM on this thread, a UIA client, and the probe it watches. Field order
/// is drop order: the probe goes first, then the client, then COM.
pub(super) struct Session {
    probe: Probe,
    automation: IUIAutomation,
    _com: Com,
}

/// Why a session could not start.
pub(super) enum Start {
    /// This host cannot take the measurement; exit 2.
    CannotVerify(String),
    /// The probe itself could not be launched.
    Failed(anyhow::Error),
}

impl Session {
    /// Initialises COM and UI Automation, then launches `probe`.
    pub(super) fn start(probe: &Path) -> Result<Self, Start> {
        Self::launch(probe, None)
    }

    /// [`Self::start`], telling a probe that reads [`RUN_FOR_ENV`] to stay
    /// up for `run_for`.
    pub(super) fn start_for(probe: &Path, run_for: Duration) -> Result<Self, Start> {
        Self::launch(probe, Some(run_for))
    }

    /// A locked, disconnected or service desktop is `CannotVerify`, found
    /// before the probe launches: no window of it could ever be read.
    fn launch(probe: &Path, run_for: Option<Duration>) -> Result<Self, Start> {
        input_desktop_name()
            .and_then(|name| interactive_desktop(&name))
            .map_err(Start::CannotVerify)?;
        let com = Com::init().map_err(|error| {
            Start::CannotVerify(format!("COM could not be initialised: {error}"))
        })?;
        // SAFETY: COM is initialised on this thread by `com`, which the
        // session drops after the client (field order).
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(
                |error| Start::CannotVerify(format!("UI Automation is not available: {error}")),
            )?;
        println!("running: {}", probe.display());
        // The desktop logger writes to stdout. An explicit probe filter must
        // preserve that stream so frame and pacing diagnostics reach the caller.
        let requested_log = std::env::var("FLUI_PROBE_RUST_LOG").ok();
        let stdout = if requested_log.is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        let log = requested_log.unwrap_or_else(|| "warn".to_owned());
        let mut command = Command::new(probe);
        if let Some(run_for) = run_for {
            command.env(RUN_FOR_ENV, run_for.as_secs().to_string());
        }
        let child = command
            .env("RUST_LOG", log)
            .stdout(stdout)
            .spawn()
            .map_err(|error| {
                Start::Failed(anyhow::anyhow!("starting {}: {error}", probe.display()))
            })?;
        Ok(Self {
            probe: Probe(child),
            automation,
            _com: com,
        })
    }

    /// The probe's window once its tree holds a button named `button`, or
    /// `None` after printing why not.
    pub(super) fn window_with_button(
        &mut self,
        button: &str,
    ) -> anyhow::Result<Option<IUIAutomationElement>> {
        let pid = self.probe.0.id();
        let found = wait_for(APPEAR_WITHIN, || {
            if let Ok(Some(status)) = self.probe.0.try_wait() {
                anyhow::bail!("the probe exited with {status} before a client found its window");
            }
            Ok(self
                .window_of(pid)?
                .filter(|window| has(&self.walk(window), BUTTON, button)))
        })?;
        if found.is_none() {
            match self.window_of(pid)? {
                Some(window) => {
                    println!("FAIL: the window's tree never showed a button named {button:?}");
                    dump(&self.walk(&window));
                }
                None => println!("FAIL: no top-level UIA element belongs to process {pid}"),
            }
        }
        Ok(found)
    }

    /// Waits up to `within` for a text named `name` under `window`: the tree
    /// in which it appeared, or `None` after dumping the last one.
    pub(super) fn wait_for_text(
        &self,
        window: &IUIAutomationElement,
        name: &str,
        within: Duration,
    ) -> anyhow::Result<Option<Vec<Node>>> {
        let tree = wait_for(within, || {
            let nodes = self.walk(window);
            Ok(has(&nodes, TEXT, name).then_some(nodes))
        })?;
        if tree.is_none() {
            println!("FAIL: no text named {name:?} within {within:?}");
            dump(&self.walk(window));
        }
        Ok(tree)
    }

    /// Observe graceful termination before Probe's forced-drop fallback.
    pub(super) fn wait_for_exit(
        &mut self,
        within: Duration,
    ) -> anyhow::Result<Option<std::process::ExitStatus>> {
        wait_for(within, || Ok(self.probe.0.try_wait()?))
    }

    /// The raw view under `root`, depth first. The raw view is used rather
    /// than the control view so that nothing the adapter published is
    /// filtered out before the check sees it.
    pub(super) fn walk(&self, root: &IUIAutomationElement) -> Vec<Node> {
        let mut nodes = Vec::new();
        // SAFETY: a plain client call on this session's COM thread.
        if let Ok(walker) = unsafe { self.automation.RawViewWalker() } {
            visit(&walker, root.clone(), 0, &mut nodes);
        }
        nodes
    }

    /// The top-level UIA element owned by `pid`, if it has one yet.
    fn window_of(&self, pid: u32) -> anyhow::Result<Option<IUIAutomationElement>> {
        let pid =
            i32::try_from(pid).map_err(|_| anyhow::anyhow!("process id {pid} out of range"))?;
        // SAFETY: plain client calls on this session's COM thread.
        unsafe {
            let root = self.automation.GetRootElement()?;
            let condition = self
                .automation
                .CreatePropertyCondition(UIA_ProcessIdPropertyId, &VARIANT::from(pid))?;
            // A null result is how UIA says "no match", and the binding
            // reports a null interface as an error.
            Ok(root.FindFirst(TreeScope_Children, &condition).ok())
        }
    }
}

/// The first node with this control type and name.
pub(super) fn find<'a>(
    nodes: &'a [Node],
    control: UIA_CONTROLTYPE_ID,
    name: &str,
) -> Option<&'a Node> {
    nodes
        .iter()
        .find(|node| node.control == control && node.name == name)
}

pub(super) fn has(nodes: &[Node], control: UIA_CONTROLTYPE_ID, name: &str) -> bool {
    find(nodes, control, name).is_some()
}

/// Whether the element has the keyboard focus, as UIA reports it to a screen
/// reader.
pub(super) fn has_keyboard_focus(element: &IUIAutomationElement) -> bool {
    // SAFETY: a plain client call on the session's COM thread.
    unsafe { element.CurrentHasKeyboardFocus() }.is_ok_and(windows::core::BOOL::as_bool)
}

/// The element's bounding rectangle in physical screen pixels.
pub(super) fn bounds(element: &IUIAutomationElement) -> anyhow::Result<RECT> {
    // SAFETY: a plain client call on the session's COM thread.
    Ok(unsafe { element.CurrentBoundingRectangle() }?)
}

pub(super) fn dump(nodes: &[Node]) {
    for node in nodes {
        let kind = if node.control == BUTTON {
            "Button".to_owned()
        } else if node.control == TEXT {
            "Text".to_owned()
        } else {
            format!("control {}", node.control.0)
        };
        let focus = if has_keyboard_focus(&node.element) {
            " [keyboard focus]"
        } else {
            ""
        };
        println!(
            "{}{kind} name={:?}{focus}",
            "  ".repeat(node.depth),
            node.name
        );
    }
}

/// Polls `probe` until it yields a value or `within` elapses.
fn wait_for<T>(
    within: Duration,
    mut probe: impl FnMut() -> anyhow::Result<Option<T>>,
) -> anyhow::Result<Option<T>> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(value) = probe()? {
            return Ok(Some(value));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

fn visit(
    walker: &IUIAutomationTreeWalker,
    element: IUIAutomationElement,
    depth: usize,
    nodes: &mut Vec<Node>,
) {
    // SAFETY: `element` is a live element of this thread's client; a
    // property the provider cannot answer reads as its empty default.
    let (control, name) = unsafe {
        (
            element.CurrentControlType().unwrap_or_default(),
            element
                .CurrentName()
                .map(|name| name.to_string())
                .unwrap_or_default(),
        )
    };
    // SAFETY: as above. A null first child or next sibling is the end of
    // that level, which the binding reports as an error.
    let mut child = unsafe { walker.GetFirstChildElement(&element) }.ok();
    nodes.push(Node {
        element,
        control,
        name,
        depth,
    });
    while let Some(current) = child {
        // SAFETY: as above.
        child = unsafe { walker.GetNextSiblingElement(&current) }.ok();
        visit(walker, current, depth + 1, nodes);
    }
}

/// COM for this thread, torn down when dropped.
struct Com;

impl Com {
    fn init() -> windows::core::Result<Self> {
        // SAFETY: paired with `CoUninitialize` in `Drop`, on the same thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        Ok(Self)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: this thread's successful `CoInitializeEx` in `init`.
        unsafe { CoUninitialize() };
    }
}

/// The name of this session's input desktop, or why it cannot be opened: a
/// service session has none, and a locked workstation refuses it or
/// switches input to `Winlogon`.
fn input_desktop_name() -> Result<String, String> {
    // SAFETY: plain Win32 call; the handle it returns is closed below.
    let desktop = unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) }
        .map_err(|error| format!("no input desktop is available (locked or headless?): {error}"))?;
    let mut name = [0_u16; 64];
    let mut needed = 0;
    // SAFETY: `desktop` was opened above; the buffer pointer and its byte
    // length describe the same live array.
    let named = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            u32::try_from(std::mem::size_of_val(&name)).expect("BUG: 128 bytes fit in u32"),
            Some(&raw mut needed),
        )
    };
    // SAFETY: `desktop` was opened above and is not used after this.
    let _ = unsafe { CloseDesktop(desktop) };
    named.map_err(|error| format!("the input desktop's name is unreadable: {error}"))?;
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..len]))
}

/// `Ok` for the interactive `Default` desktop, where a probe's window and
/// its UIA tree appear; otherwise why the check cannot take its measurement.
fn interactive_desktop(name: &str) -> Result<(), String> {
    if name.eq_ignore_ascii_case("Default") {
        Ok(())
    } else {
        Err(format!(
            "the input desktop is {name:?}, not the interactive desktop (workstation locked?)"
        ))
    }
}

/// The probe process, killed when dropped so no window outlives the check.
struct Probe(Child);

impl Drop for Probe {
    fn drop(&mut self) {
        // The probe may already have quit on its own timer.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the interactive desktop admits a measurement; a locked
    /// workstation's `Winlogon` desktop or a screen saver's is CANNOT VERIFY.
    #[test]
    fn only_the_interactive_desktop_admits_a_measurement() {
        let rows = [
            ("Default", true),
            ("default", true),
            ("Winlogon", false),
            ("Screen-saver", false),
            ("", false),
        ];
        let failed: Vec<_> = rows
            .iter()
            .filter(|&&(name, admitted)| interactive_desktop(name).is_ok() != admitted)
            .map(|&(name, _)| name)
            .collect();
        assert!(failed.is_empty(), "desktop rows failed: {failed:?}");
    }
}
