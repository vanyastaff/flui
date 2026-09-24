//! A UI Automation client over one launched probe, shared by the Windows
//! checks: what Narrator sees of a window, read through the same API.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
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
const APPEAR_WITHIN: Duration = Duration::from_secs(15);
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
        // `FLUI_PROBE_RUST_LOG` raises the probe's own logging (its stderr is
        // this terminal) without touching xtask's.
        let log = std::env::var("FLUI_PROBE_RUST_LOG").unwrap_or_else(|_| "warn".to_owned());
        let child = Command::new(probe)
            .env("RUST_LOG", log)
            .stdout(Stdio::null())
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

/// The probe process, killed when dropped so no window outlives the check.
struct Probe(Child);

impl Drop for Probe {
    fn drop(&mut self) {
        // The probe may already have quit on its own timer.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
