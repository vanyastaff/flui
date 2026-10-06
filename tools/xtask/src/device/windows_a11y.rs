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
//! The disclosure and range fixtures additionally require their UIA patterns,
//! properties and visible sibling text to agree after native actions. These
//! are synthetic semantics fixtures, not a widget catalog or Narrator session.
//!
//! Exit 0 on PASS, 1 on FAIL (with the tree dumped), 2 when this host cannot
//! take the measurement (UI Automation could not be instantiated).

use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::UI::Accessibility::{
    ExpandCollapseState, ExpandCollapseState_Collapsed, ExpandCollapseState_Expanded,
    IUIAutomationElement, IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
    IUIAutomationRangeValuePattern, UIA_CONTROLTYPE_ID, UIA_E_INVALIDOPERATION,
    UIA_ExpandCollapsePatternId, UIA_InvokePatternId, UIA_RangeValuePatternId,
    UIA_SliderControlTypeId,
};
use windows::core::HRESULT;

use super::uia::{self, BUTTON, Session, Start, TEXT};

/// What the counter template shows, and so what a screen reader must speak.
pub(super) const PROMPT: &str = "You have pushed the button this many times:";
pub(super) const INCREMENT: &str = "Increment";

/// How long the count gets to advance after an action.
pub(super) const ADVANCE_WITHIN: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);
const UNCHANGED_FOR: Duration = Duration::from_secs(1);
/// The longest [`drive`] waits before its verdict, wait by wait: the window's
/// appearance, the count's first advance, the initial pattern state, three
/// disclosure transitions (each a wait and an unchanged window after its
/// refused repeat), two range values, the unchanged window after the rejected
/// value, the counter barrier and the last range value. A wait added to
/// `drive` is added here.
const CHECK_DEADLINE: Duration = Duration::from_secs(
    uia::APPEAR_WITHIN.as_secs()
        + ADVANCE_WITHIN.as_secs() * 2
        + (ADVANCE_WITHIN.as_secs() + UNCHANGED_FOR.as_secs()) * 3
        + ADVANCE_WITHIN.as_secs() * 2
        + UNCHANGED_FOR.as_secs()
        + ADVANCE_WITHIN.as_secs() * 2,
);
/// How long the probe is told to stay up: twice [`CHECK_DEADLINE`], leaving
/// the tree walks between waits room, so the probe quitting on its own can
/// never read as a FAIL during the last wait. The session kills the probe
/// when the check ends, so the margin costs nothing.
const PROBE_RUN_FOR: Duration = Duration::from_secs(CHECK_DEADLINE.as_secs() * 2);
const DISCLOSURE: &str = "Probe disclosure";
const RANGE: &str = "Probe numeric range";

/// Runs the check against the built probe and returns its exit code.
pub(super) fn run(probe: &Path) -> anyhow::Result<u8> {
    let mut session = match Session::start_for(probe, PROBE_RUN_FOR) {
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
    match drive_patterns(session, &window) {
        Ok(passed) => Ok(passed),
        Err(error) => {
            println!("FAIL: native pattern or property request failed: {error:#}");
            uia::dump(&session.walk(&window));
            Ok(false)
        }
    }
}

fn drive_patterns(session: &Session, window: &IUIAutomationElement) -> anyhow::Result<bool> {
    if !wait_for_state(session, window, "initial disclosure and range", |nodes| {
        let (Some(disclosure), Some(range)) = (disclosure_pattern(nodes)?, range_pattern(nodes)?)
        else {
            return Ok(false);
        };
        // SAFETY: live UIA patterns on the session's COM thread. Read current
        // properties rather than cached properties from an earlier frame.
        let valid = unsafe {
            disclosure.CurrentExpandCollapseState()? == ExpandCollapseState_Collapsed
                && range.CurrentValue()?.to_bits() == 0.0_f64.to_bits()
                && range.CurrentMinimum()?.to_bits() == 0.0_f64.to_bits()
                && range.CurrentMaximum()?.to_bits() == 10.0_f64.to_bits()
                && range.CurrentSmallChange()?.to_bits() == 1.0_f64.to_bits()
                && range.CurrentLargeChange()?.to_bits() == 1.0_f64.to_bits()
                && !range.CurrentIsReadOnly()?.as_bool()
        };
        Ok(valid
            && uia::has(nodes, TEXT, "Disclosure state: collapsed")
            && uia::has(nodes, TEXT, "Published range value: 0"))
    })? {
        return Ok(false);
    }
    println!("UIA patterns ready: disclosure collapsed; writable range 0..10, value 0, step 1");

    for (expanded, text) in [
        (true, "Disclosure state: expanded"),
        (false, "Disclosure state: collapsed"),
        (true, "Disclosure state: expanded"),
    ] {
        let pattern = disclosure_pattern(&session.walk(window))?
            .ok_or_else(|| anyhow::anyhow!("disclosure disappeared before its action"))?;
        // SAFETY: live pattern on the session's COM thread; no arguments.
        unsafe {
            if expanded {
                pattern.Expand()?;
            } else {
                pattern.Collapse()?;
            }
        }
        let expected = if expanded {
            ExpandCollapseState_Expanded
        } else {
            ExpandCollapseState_Collapsed
        };
        if !wait_for_state(session, window, text, |nodes| {
            disclosure_matches(nodes, expected, text)
        })? {
            return Ok(false);
        }
        println!("UIA disclosure: {text}");

        // AccessKit 0.35.1 refuses a request for the already published state.
        // Require that exact HRESULT, then watch both state and visible text;
        // successful enqueue alone would not establish an idempotent result.
        let pattern = disclosure_pattern(&session.walk(window))?
            .ok_or_else(|| anyhow::anyhow!("disclosure disappeared before its repeat"))?;
        // SAFETY: as above.
        let repeated = unsafe {
            if expanded {
                pattern.Expand()
            } else {
                pattern.Collapse()
            }
        };
        match repeated {
            Err(error) if error.code() == HRESULT(UIA_E_INVALIDOPERATION.cast_signed()) => {}
            result => {
                println!(
                    "FAIL: repeated disclosure request must return UIA_E_INVALIDOPERATION: {result:?}"
                );
                return Ok(false);
            }
        }
        if !require_unchanged(session, window, text, |nodes| {
            disclosure_matches(nodes, expected, text)
        })? {
            return Ok(false);
        }
        println!("UIA disclosure repeat refused without changing {text:?}");
    }

    for value in [2.375, 10.0] {
        if !set_range_and_wait(session, window, value)? {
            return Ok(false);
        }
    }
    let range = range_pattern(&session.walk(window))?
        .ok_or_else(|| anyhow::anyhow!("range disappeared before its rejected value"))?;
    // SAFETY: live pattern on the COM thread; the plain f64 is intentional.
    // AccessKit queues this request and returns success. The current owner
    // rejects 11; only unchanged state/text proves that downstream rejection.
    unsafe { range.SetValue(11.0) }?;
    if !require_unchanged(session, window, "out-of-range 11 refused", |nodes| {
        range_matches(nodes, 10.0, "Published range value: 10")
    })? {
        return Ok(false);
    }
    println!("UIA range: out-of-range 11 left value 10 unchanged");
    let nodes = session.walk(window);
    let increment = unique_element(&nodes, BUTTON, INCREMENT)?
        .ok_or_else(|| anyhow::anyhow!("Increment disappeared before the rejection barrier"))?;
    // SAFETY: live element and pattern on the session's COM thread. This
    // independent action must produce a frame before another valid numeric
    // request could conceal a delayed, incorrectly accepted value of 11.
    unsafe {
        increment
            .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?
            .Invoke()?;
    }
    if !wait_for_state(
        session,
        window,
        "count 2 with rejected range still 10",
        |nodes| {
            Ok(uia::has(nodes, TEXT, "2")
                && range_matches(nodes, 10.0, "Published range value: 10")?)
        },
    )? {
        return Ok(false);
    }
    println!("UIA rejection barrier: count 2 and exact range value 10");
    if !set_range_and_wait(session, window, 4.625)? {
        return Ok(false);
    }
    if !uia::has(&session.walk(window), TEXT, "2") {
        println!("FAIL: disclosure or range actions changed the independent counter");
        uia::dump(&session.walk(window));
        return Ok(false);
    }
    Ok(true)
}

fn unique_element<'a>(
    nodes: &'a [uia::Node],
    control: UIA_CONTROLTYPE_ID,
    name: &str,
) -> anyhow::Result<Option<&'a IUIAutomationElement>> {
    let mut matches = nodes
        .iter()
        .filter(|node| node.control == control && node.name == name);
    let first = matches.next();
    anyhow::ensure!(
        matches.next().is_none(),
        "duplicate UIA control named {name:?}"
    );
    Ok(first.map(|node| &node.element))
}

fn disclosure_pattern(
    nodes: &[uia::Node],
) -> anyhow::Result<Option<IUIAutomationExpandCollapsePattern>> {
    let Some(element) = unique_element(nodes, BUTTON, DISCLOSURE)? else {
        return Ok(None);
    };
    // SAFETY: live element on the session's COM thread.
    Ok(Some(unsafe {
        element.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
            UIA_ExpandCollapsePatternId,
        )?
    }))
}

fn range_pattern(nodes: &[uia::Node]) -> anyhow::Result<Option<IUIAutomationRangeValuePattern>> {
    let Some(element) = unique_element(nodes, UIA_SliderControlTypeId, RANGE)? else {
        return Ok(None);
    };
    // SAFETY: live element on the session's COM thread.
    Ok(Some(unsafe {
        element.GetCurrentPatternAs::<IUIAutomationRangeValuePattern>(UIA_RangeValuePatternId)?
    }))
}

fn disclosure_matches(
    nodes: &[uia::Node],
    state: ExpandCollapseState,
    text: &str,
) -> anyhow::Result<bool> {
    let Some(pattern) = disclosure_pattern(nodes)? else {
        return Ok(false);
    };
    // SAFETY: live pattern on the session's COM thread.
    Ok(unsafe { pattern.CurrentExpandCollapseState()? } == state && uia::has(nodes, TEXT, text))
}

fn range_matches(nodes: &[uia::Node], value: f64, text: &str) -> anyhow::Result<bool> {
    let Some(pattern) = range_pattern(nodes)? else {
        return Ok(false);
    };
    // SAFETY: live pattern on the session's COM thread. All tested fractions
    // are exactly representable in binary; no tolerance hides quantization.
    Ok(
        unsafe { pattern.CurrentValue()? }.to_bits() == value.to_bits()
            && uia::has(nodes, TEXT, text),
    )
}

fn set_range_and_wait(
    session: &Session,
    window: &IUIAutomationElement,
    value: f64,
) -> anyhow::Result<bool> {
    let range = range_pattern(&session.walk(window))?
        .ok_or_else(|| anyhow::anyhow!("range disappeared before SetValue"))?;
    // SAFETY: live pattern on the COM thread; value is an admitted finite f64.
    unsafe { range.SetValue(value) }?;
    let text = format!("Published range value: {value}");
    let passed = wait_for_state(session, window, &text, |nodes| {
        range_matches(nodes, value, &text)
    })?;
    if passed {
        println!("UIA range: exact value {value}");
    }
    Ok(passed)
}

fn wait_for_state(
    session: &Session,
    window: &IUIAutomationElement,
    description: &str,
    mut predicate: impl FnMut(&[uia::Node]) -> anyhow::Result<bool>,
) -> anyhow::Result<bool> {
    let deadline = Instant::now() + ADVANCE_WITHIN;
    loop {
        let nodes = session.walk(window);
        if predicate(&nodes)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            println!("FAIL: {description} not published within {ADVANCE_WITHIN:?}");
            uia::dump(&nodes);
            return Ok(false);
        }
        std::thread::sleep(POLL);
    }
}

fn require_unchanged(
    session: &Session,
    window: &IUIAutomationElement,
    description: &str,
    mut predicate: impl FnMut(&[uia::Node]) -> anyhow::Result<bool>,
) -> anyhow::Result<bool> {
    let deadline = Instant::now() + UNCHANGED_FOR;
    loop {
        let nodes = session.walk(window);
        if !predicate(&nodes)? {
            println!("FAIL: {description} changed during {UNCHANGED_FOR:?} observation");
            uia::dump(&nodes);
            return Ok(false);
        }
        if Instant::now() >= deadline {
            return Ok(true);
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe outlives every wait of the check, and it reads the lifetime
    /// the session hands it: a probe on a fixed timer shorter than the
    /// check's waits quits during the last one, which reads as a FAIL.
    #[test]
    fn the_probe_stays_up_past_the_checks_deadline() {
        assert!(PROBE_RUN_FOR > CHECK_DEADLINE);
        let probe = include_str!("../../../../examples/a11y_probe.rs");
        assert!(
            probe.contains(&format!("{:?}", uia::RUN_FOR_ENV)),
            "examples/a11y_probe.rs must read {} for its lifetime",
            uia::RUN_FOR_ENV
        );
    }
}
