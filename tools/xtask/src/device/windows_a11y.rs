//! `windows-a11y`: the generated counter driven through UI Automation, the
//! API Narrator, NVDA and JAWS read a window through.
//!
//! The probe (`examples/a11y_probe.rs`, built with the facade's `a11y`
//! feature) is launched on a real window and this process acts as the
//! assistive technology: it finds the window by the probe's process id,
//! walks its raw UIA tree, requires the prompt, the count and the button to
//! be there under the names a screen reader would speak, invokes the button
//! through `IUIAutomationInvokePattern` and waits for the count's name to
//! advance. No pointer or keyboard event is synthesised anywhere; if the
//! count advances, a Narrator user could press the button.
//!
//! The oracle is names, not presence: a text node with an empty name is
//! present in the tree and silent to a screen reader, which is exactly what
//! the first run of this check found.
//!
//! Exit 0 on PASS, 1 on FAIL (with the tree dumped), 2 when this host cannot
//! take the measurement (UI Automation could not be instantiated).

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationTreeWalker, TreeScope_Children, UIA_ButtonControlTypeId, UIA_CONTROLTYPE_ID,
    UIA_InvokePatternId, UIA_ProcessIdPropertyId, UIA_TextControlTypeId,
};

/// What the counter template shows, and so what a screen reader must speak.
const PROMPT: &str = "You have pushed the button this many times:";
const BUTTON: &str = "Increment";
const BEFORE: &str = "0";
const AFTER: &str = "1";

/// How long the probe gets to put its window up and publish a first tree.
/// The first query is also what activates the adapter, so the tree can
/// trail the window by a frame.
const APPEAR_WITHIN: Duration = Duration::from_secs(15);
/// How long the count gets to advance after the invoke.
const ADVANCE_WITHIN: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);

/// Runs the check against the built probe and returns its exit code.
pub(super) fn run(probe: &Path) -> anyhow::Result<u8> {
    let _com = match Com::init() {
        Ok(com) => com,
        Err(error) => {
            println!("CANNOT_VERIFY: COM could not be initialised: {error}");
            return Ok(2);
        }
    };
    // SAFETY: COM is initialised on this thread for as long as `_com` lives,
    // which outlives every interface created below.
    let automation: IUIAutomation =
        match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
            Ok(automation) => automation,
            Err(error) => {
                println!("CANNOT_VERIFY: UI Automation is not available: {error}");
                return Ok(2);
            }
        };

    println!("running: {}", probe.display());
    let mut child = Probe(
        Command::new(probe)
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .spawn()
            .map_err(|error| anyhow::anyhow!("starting {}: {error}", probe.display()))?,
    );
    let verdict = drive(&automation, &mut child.0);
    drop(child);
    let passed = verdict?;
    println!("A11Y={}", if passed { "PASS" } else { "FAIL" });
    Ok(u8::from(!passed))
}

/// The probe's side of the check: `Ok(true)` on PASS, `Ok(false)` on FAIL
/// after printing why.
fn drive(automation: &IUIAutomation, child: &mut Child) -> anyhow::Result<bool> {
    let pid = child.id();
    let Some(window) = wait_for(APPEAR_WITHIN, || {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow::anyhow!(
                "the probe exited with {status} before a client found its window"
            ));
        }
        let tree = window_of(automation, pid)?.map(|window| {
            let nodes = walk(automation, &window);
            (window, nodes)
        });
        Ok(tree.filter(|(_, nodes)| has(nodes, UIA_ButtonControlTypeId, BUTTON)))
    })?
    else {
        match window_of(automation, pid)? {
            Some(window) => {
                println!("FAIL: the window's tree never showed a button named {BUTTON:?}");
                dump(&walk(automation, &window));
            }
            None => println!("FAIL: no top-level UIA element belongs to process {pid}"),
        }
        return Ok(false);
    };
    let (window, before) = window;
    println!("tree before the invoke:");
    dump(&before);

    let mut missing = Vec::new();
    for (control, name) in [
        (UIA_TextControlTypeId, PROMPT),
        (UIA_TextControlTypeId, BEFORE),
    ] {
        if !has(&before, control, name) {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        println!("FAIL: no text named {missing:?}: a screen reader would not speak it");
        return Ok(false);
    }

    let button = before
        .iter()
        .find(|node| node.control == UIA_ButtonControlTypeId && node.name == BUTTON)
        .map(|node| node.element.clone())
        .expect("BUG: the wait above returned only a tree holding the button");
    // SAFETY: `button` is a live element of this thread's UIA client.
    let invoke =
        unsafe { button.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) };
    let invoked = invoke.and_then(|pattern| {
        // SAFETY: as above; `Invoke` takes no arguments.
        unsafe { pattern.Invoke() }
    });
    if let Err(error) = invoked {
        println!("FAIL: the button does not accept Invoke: {error}");
        return Ok(false);
    }
    println!("invoked {BUTTON:?}");

    let advanced = wait_for(ADVANCE_WITHIN, || {
        let nodes = walk(automation, &window);
        Ok(has(&nodes, UIA_TextControlTypeId, AFTER).then_some(nodes))
    })?;
    if let Some(after) = advanced {
        println!("tree after the invoke:");
        dump(&after);
        Ok(true)
    } else {
        println!("FAIL: no text named {AFTER:?} within {ADVANCE_WITHIN:?} of the invoke");
        dump(&walk(automation, &window));
        Ok(false)
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

/// The top-level UIA element owned by `pid`, if it has one yet.
fn window_of(automation: &IUIAutomation, pid: u32) -> anyhow::Result<Option<IUIAutomationElement>> {
    let pid = i32::try_from(pid).map_err(|_| anyhow::anyhow!("process id {pid} out of range"))?;
    // SAFETY: plain UIA client calls on this thread's COM apartment.
    unsafe {
        let root = automation.GetRootElement()?;
        let condition =
            automation.CreatePropertyCondition(UIA_ProcessIdPropertyId, &VARIANT::from(pid))?;
        // A null result is how UIA says "no match", and the binding reports
        // a null interface as an error.
        Ok(root.FindFirst(TreeScope_Children, &condition).ok())
    }
}

/// One node of the raw view, as an assistive technology reads it.
struct Node {
    element: IUIAutomationElement,
    control: UIA_CONTROLTYPE_ID,
    name: String,
    depth: usize,
}

/// The raw view under `root`, depth first. The raw view is used rather than
/// the control view so that nothing the adapter published is filtered out
/// before the check sees it.
fn walk(automation: &IUIAutomation, root: &IUIAutomationElement) -> Vec<Node> {
    let mut nodes = Vec::new();
    // SAFETY: plain UIA client call on this thread's COM apartment.
    if let Ok(walker) = unsafe { automation.RawViewWalker() } {
        visit(&walker, root.clone(), 0, &mut nodes);
    }
    nodes
}

fn visit(
    walker: &IUIAutomationTreeWalker,
    element: IUIAutomationElement,
    depth: usize,
    nodes: &mut Vec<Node>,
) {
    // SAFETY: `element` is a live element of this thread's UIA client; a
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

fn has(nodes: &[Node], control: UIA_CONTROLTYPE_ID, name: &str) -> bool {
    nodes
        .iter()
        .any(|node| node.control == control && node.name == name)
}

fn dump(nodes: &[Node]) {
    for node in nodes {
        let kind = if node.control == UIA_ButtonControlTypeId {
            "Button".to_owned()
        } else if node.control == UIA_TextControlTypeId {
            "Text".to_owned()
        } else {
            format!("control {}", node.control.0)
        };
        println!("{}{kind} name={:?}", "  ".repeat(node.depth), node.name);
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
