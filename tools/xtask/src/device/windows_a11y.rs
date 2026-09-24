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
use std::time::Duration;

use windows::Win32::UI::Accessibility::{IUIAutomationInvokePattern, UIA_InvokePatternId};

use super::uia::{self, BUTTON, Session, Start, TEXT};

/// What the counter template shows, and so what a screen reader must speak.
pub(super) const PROMPT: &str = "You have pushed the button this many times:";
pub(super) const INCREMENT: &str = "Increment";

/// How long the count gets to advance after an action.
pub(super) const ADVANCE_WITHIN: Duration = Duration::from_secs(5);

/// Runs the check against the built probe and returns its exit code.
pub(super) fn run(probe: &Path) -> anyhow::Result<u8> {
    let mut session = match Session::start(probe) {
        Ok(session) => session,
        Err(Start::CannotVerify(why)) => {
            println!("CANNOT_VERIFY: {why}");
            return Ok(2);
        }
        Err(Start::Failed(error)) => return Err(error),
    };
    let passed = drive(&mut session)?;
    drop(session);
    println!("A11Y={}", if passed { "PASS" } else { "FAIL" });
    Ok(u8::from(!passed))
}

/// `Ok(true)` on PASS, `Ok(false)` on FAIL after printing why.
fn drive(session: &mut Session) -> anyhow::Result<bool> {
    let Some(window) = session.window_with_button(INCREMENT)? else {
        return Ok(false);
    };
    let before = session.walk(&window);
    println!("tree before the invoke:");
    uia::dump(&before);

    let missing: Vec<_> = [PROMPT, "0"]
        .into_iter()
        .filter(|name| !uia::has(&before, TEXT, name))
        .collect();
    if !missing.is_empty() {
        println!("FAIL: no text named {missing:?}: a screen reader would not speak it");
        return Ok(false);
    }

    let button = &uia::find(&before, BUTTON, INCREMENT)
        .expect("BUG: window_with_button returns only a window whose tree holds the button")
        .element;
    // SAFETY: `button` is a live element of this session's client, on its
    // COM thread.
    let invoked =
        unsafe { button.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
            .and_then(|pattern| {
                // SAFETY: as above; `Invoke` takes no arguments.
                unsafe { pattern.Invoke() }
            });
    if let Err(error) = invoked {
        println!("FAIL: the button does not accept Invoke: {error}");
        return Ok(false);
    }
    println!("invoked {INCREMENT:?}");

    let Some(after) = session.wait_for_text(&window, "1", ADVANCE_WITHIN)? else {
        return Ok(false);
    };
    println!("tree after the invoke:");
    uia::dump(&after);
    Ok(true)
}
