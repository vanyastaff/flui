//! What every window backend promises about its state and commands
//! (ADR-0237, ADR-0254), checked from outside.
//!
//! A backend runs [`assert_conforms`] against a [`WindowFixture`] of its window.
//! The checks drive the window only through [`PlatformWindow`], admitting each
//! command against the capabilities of the window's snapshot the way the
//! application's window queue does, so a backend cannot pass by being tested
//! against its own assumptions:
//!
//! - **Capabilities are honest.** Admission refuses a command as
//!   [`Unsupported`](AdmissionError::Unsupported) exactly when its capability
//!   is `Unavailable`, for good — its reason is not one worth asking again
//!   ([`Unsupported::is_retryable`](flui_platform::Unsupported::is_retryable))
//!   — and the window applies every command its capabilities admitted: never
//!   claimed and then refused.
//! - **State converges.** A command the window accepted shows in its
//!   [`state`](PlatformWindow::state) once the window system has settled: the
//!   title, size, position, mode, visibility, flags and constraints it asked for,
//!   and the window system draws the material it asked for.
//! - **Revisions mean change.** Reading the state twice with nothing between
//!   keeps its revision; a command that changed something raises it.
//! - **Bad numbers never reach the window system.** A size that is not finite
//!   and positive is refused at admission as
//!   [`InvalidGeometry`](AdmissionError::InvalidGeometry), so no window sees it.
//! - **Verbs answer to their capability.** The frame-rate hint and the input
//!   regions, which a window takes outside the command queue because they are
//!   sent every frame, answer `Unsupported` exactly when their capability is
//!   `Unavailable` (ADR-0254 §3). The material is a command like the others.
//! - **A closed window refuses.** Every admitted command after the window
//!   closed — through [`WindowCommand::Close`] where the window takes it, else
//!   the way the window system closes it — is [`Closed`](ApplyError::Closed).
//! - **The snapshot agrees with the window system.** Once the window system
//!   has settled, each fact the fixture reads back by its own means (the
//!   drawable metrics and scale, focus, visibility) is the snapshot's
//!   (ADR-0254 §1). The capabilities the checks read are the snapshot's.
//!
//! [`check`] returns each violated promise by name; [`assert_conforms`] panics
//! listing them.

use std::rc::Rc;

use flui_foundation::geometry::{Point, Rect, Size, WindowMetrics};
use flui_platform::input_regions::{InputRegion, InputRegionKind};
use flui_platform::{
    AdmissionError, ApplyError, Availability, FrameRateHint, PlatformWindow, SizeConstraints,
    Unsupported, WindowCommand, WindowMaterial, WindowMode, WindowState,
};

use crate::window_commands::AdmitAndApply;

/// What the window system says of the window now, read by the fixture by its
/// own means (the operating system's queries, a headless host's model), never
/// through the window's snapshot. Every fact is read: a fixture cannot leave
/// one out and pass for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readback {
    /// The drawable area in device pixels and its scale; `None` while the
    /// window has no drawable area (minimized), whose snapshot keeps the last.
    pub metrics: Option<WindowMetrics>,
    /// Whether the window has input focus.
    pub focused: bool,
    /// Whether the window is the active window of its desktop.
    pub active: bool,
    /// Whether the window is shown and nothing (a cloak, a locked session)
    /// hides it.
    pub visible: bool,
    /// The material the window system draws behind the window; `None` where
    /// it draws one the contract has no name for, or none can be read. Not a
    /// fact of the snapshot: the checks compare it with the
    /// [`WindowCommand::SetMaterial`] the window accepted.
    pub material: Option<WindowMaterial>,
}

/// A window under test, as the checks drive it.
pub trait WindowFixture {
    /// The window under test, open and visible, in normal mode.
    fn window(&self) -> Rc<dyn PlatformWindow>;

    /// What the window system says of the window now.
    fn readback(&self) -> Readback;

    /// Let the window system carry out what was accepted: pump the event loop
    /// until it is quiet. A window that applies commands synchronously keeps
    /// this empty.
    fn settle(&mut self);

    /// Close the window the way the window system would, and settle.
    fn close(&mut self);

    /// Make the window system revise what the window honours, leaving the
    /// title command honoured, and settle; `false` where the window system
    /// gives a test no such lever, which skips the check that needs one.
    fn revise_capabilities(&mut self) -> bool {
        false
    }

    /// Whether the window's input path honours input regions, as its
    /// [`InputCapabilities::input_regions`](flui_platform::input_capabilities::InputCapabilities::input_regions)
    /// reports it: the capability the input-region verb answers to, which
    /// the snapshot does not carry.
    fn input_regions(&self) -> Availability;

    /// Whether the window has a native surface, which its surface target
    /// hands out a window handle for while it is open, on the thread the
    /// checks run on (ADR-0254 §5); a headless window has none.
    fn has_native_surface(&self) -> bool;
}

/// One promise a window broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The check that caught it.
    pub check: &'static str,
    /// What was expected and what the window did.
    pub detail: String,
}

/// Run every check against `fixture` and return what the window broke, in the
/// order the checks ran. An empty list is a conforming window.
///
/// The checks close the window last, so the fixture is spent afterwards.
pub fn check(fixture: &mut dyn WindowFixture) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (name, run) in CHECKS {
        if let Err(detail) = run(fixture) {
            violations.push(Violation {
                check: name,
                detail,
            });
        }
    }
    violations
}

/// Panic, listing each check the window failed.
///
/// # Panics
/// When [`check`] finds a violation.
pub fn assert_conforms(fixture: &mut dyn WindowFixture) {
    let violations = check(fixture);
    assert!(
        violations.is_empty(),
        "the window broke {} promise(s):\n{}",
        violations.len(),
        violations
            .iter()
            .map(|violation| format!("  {}: {}", violation.check, violation.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

type Check = fn(&mut dyn WindowFixture) -> Result<(), String>;

const CHECKS: &[(&str, Check)] = &[
    ("capabilities_are_honest", capabilities_are_honest),
    ("accepted_commands_converge", accepted_commands_converge),
    ("revisions_mean_change", revisions_mean_change),
    ("bad_numbers_are_refused", bad_numbers_are_refused),
    (
        "the_snapshot_agrees_with_the_window_system",
        the_snapshot_agrees_with_the_window_system,
    ),
    (
        "a_command_admitted_before_a_revision_is_superseded",
        a_command_admitted_before_a_revision_is_superseded,
    ),
    (
        "per_frame_verbs_report_unsupported_exactly_when_unavailable",
        per_frame_verbs_report_unsupported_exactly_when_unavailable,
    ),
    (
        "a_surface_target_is_send_sync_and_unavailable_after_close",
        a_surface_target_is_send_sync_and_unavailable_after_close,
    ),
    ("a_closed_window_refuses", a_closed_window_refuses),
];

/// Every command kind, with numbers a window of any size can take, built from
/// the window's current state so toggles actually toggle.
fn sample_commands(state: &WindowState) -> Vec<WindowCommand> {
    vec![
        WindowCommand::SetTitle("window conformance".to_owned()),
        WindowCommand::SetPosition(Point::new(120.0, 90.0)),
        WindowCommand::SetSize(Size::new(640.0, 480.0)),
        WindowCommand::SetConstraints(SizeConstraints::new(
            Some(Size::new(200.0, 150.0)),
            Some(Size::new(2000.0, 1500.0)),
        )),
        // The frame first: a window system that resizes a window only through
        // its frame (Win32) takes resizing back only on a framed window.
        WindowCommand::SetDecorated(!state.decorated),
        WindowCommand::SetResizable(!state.resizable),
        WindowCommand::SetMode(WindowMode::Minimized),
        WindowCommand::SetMode(WindowMode::Maximized),
        WindowCommand::SetMode(WindowMode::Fullscreen),
        WindowCommand::SetMode(WindowMode::Normal),
        WindowCommand::Hide,
        WindowCommand::Show,
        WindowCommand::Activate,
        WindowCommand::SetMaterial(WindowMaterial::System),
        WindowCommand::SetMaterial(WindowMaterial::Opaque),
    ]
}

fn capabilities_are_honest(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let mut broken = Vec::new();
    for command in sample_commands(&window.state()) {
        let capabilities = window.state().capabilities;
        let capable = capabilities.availability(&command).is_available();
        match (capable, command.clone().admit(&capabilities)) {
            (false, Err(AdmissionError::Unsupported(refused))) if refused.is_retryable() => {
                broken.push(format!(
                    "{command:?} is not supported but was refused as {refused}, which asks to be \
                     tried again"
                ));
            }
            (false, Err(AdmissionError::Unsupported(_))) => {}
            (false, other) => broken.push(format!(
                "{command:?} is not supported but admission answered {other:?}, not Unsupported"
            )),
            (true, Err(refused)) => {
                broken.push(format!(
                    "{command:?} is supported but admission refused it: {refused}"
                ));
            }
            (true, Ok(admitted)) => {
                if let Err(error) = window.apply(admitted) {
                    broken.push(format!("{command:?} is supported but answered {error:?}"));
                }
                fixture.settle();
            }
        }
    }
    finish(broken)
}

fn accepted_commands_converge(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let capabilities = window.state().capabilities;
    let mut broken = Vec::new();
    for command in sample_commands(&window.state()) {
        if window.admit_and_apply(command.clone()).is_err() {
            continue;
        }
        fixture.settle();
        let state = window.state();
        let outcome = match &command {
            WindowCommand::SetTitle(title) => (state.title.as_deref() == Some(title))
                .then_some(())
                .ok_or_else(|| format!("the title is {:?}, not {title:?}", state.title)),
            WindowCommand::SetPosition(at) => {
                if capabilities.observes_position.is_available() {
                    let there = state.outer.position;
                    there
                        .is_some_and(|there| near(there.x, at.x) && near(there.y, at.y))
                        .then_some(())
                        .ok_or_else(|| format!("the window is at {there:?}, not {at:?}"))
                } else {
                    Ok(())
                }
            }
            WindowCommand::SetSize(size) => (near(state.content_size.width, size.width)
                && near(state.content_size.height, size.height))
            .then_some(())
            .ok_or_else(|| format!("the content size is {:?}, not {size:?}", state.content_size)),
            WindowCommand::SetConstraints(constraints) => (state.constraints == *constraints)
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "the constraints are {:?}, not {constraints:?}",
                        state.constraints
                    )
                }),
            WindowCommand::SetResizable(on) => (state.resizable == *on)
                .then_some(())
                .ok_or_else(|| format!("resizable is {}, not {on}", state.resizable)),
            WindowCommand::SetDecorated(on) => (state.decorated == *on)
                .then_some(())
                .ok_or_else(|| format!("decorated is {}, not {on}", state.decorated)),
            WindowCommand::SetMode(WindowMode::Minimized) => {
                if capabilities.observes_minimized.is_available() {
                    (state.mode == WindowMode::Minimized)
                        .then_some(())
                        .ok_or_else(|| format!("the mode is {:?}, not Minimized", state.mode))
                } else {
                    Ok(())
                }
            }
            WindowCommand::SetMode(mode) => (state.mode == *mode)
                .then_some(())
                .ok_or_else(|| format!("the mode is {:?}, not {mode:?}", state.mode)),
            WindowCommand::Hide => (!state.visible)
                .then_some(())
                .ok_or_else(|| "the window is still visible".to_owned()),
            WindowCommand::Show => state
                .visible
                .then_some(())
                .ok_or_else(|| "the window is still hidden".to_owned()),
            WindowCommand::SetMaterial(material) => {
                let held = fixture.readback().material;
                (held == Some(*material))
                    .then_some(())
                    .ok_or_else(|| format!("the window system draws {held:?}, not {material:?}"))
            }
            _ => Ok(()),
        };
        if let Err(detail) = outcome {
            broken.push(format!("after {command:?}, {detail}"));
        }
    }
    finish(broken)
}

fn revisions_mean_change(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    fixture.settle();
    let first = window.state();
    let second = window.state();
    if first != second {
        return Err("two reads with nothing between them differ".to_owned());
    }
    let command = WindowCommand::SetTitle("a title nobody had".to_owned());
    if !window.state().capabilities.permits(&command) {
        return Ok(());
    }
    window
        .admit_and_apply(command)
        .map_err(|refusal| format!("SetTitle was refused: {refusal:?}"))?;
    fixture.settle();
    let after = window.state();
    if after.title == first.title {
        return Err("SetTitle changed nothing to compare revisions across".to_owned());
    }
    if after.revision <= first.revision {
        return Err(format!(
            "the state changed but its revision stayed at {:?}",
            after.revision
        ));
    }
    Ok(())
}

fn bad_numbers_are_refused(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let mut broken = Vec::new();
    for command in [
        WindowCommand::SetSize(Size::new(f64::NAN, 100.0)),
        WindowCommand::SetSize(Size::new(-5.0, 100.0)),
        WindowCommand::SetPosition(Point::new(f64::INFINITY, 0.0)),
    ] {
        // Before the capabilities: no window, whatever it honours, is handed
        // numbers that describe none.
        match command.clone().admit(&window.state().capabilities) {
            Err(AdmissionError::InvalidGeometry) => {}
            other => broken.push(format!("{command:?} was admitted as {other:?}")),
        }
    }
    finish(broken)
}

/// What `state` says that `readback` disagrees with, after `step`.
fn disagreements(step: &str, state: &WindowState, readback: Readback) -> Vec<String> {
    let mut broken = Vec::new();
    if let Some(metrics) = readback.metrics
        && state.metrics != Some(metrics)
    {
        broken.push(format!(
            "{step}: the snapshot's metrics are {:?}, the window system's {metrics:?}",
            state.metrics
        ));
    }
    for (fact, published, read) in [
        ("focused", state.focused, readback.focused),
        ("active", state.active, readback.active),
        ("visible", state.visible, readback.visible),
    ] {
        if published != read {
            broken.push(format!(
                "{step}: the snapshot says {fact} {published}, the window system {read}"
            ));
        }
    }
    broken
}

fn the_snapshot_agrees_with_the_window_system(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    let window = fixture.window();
    fixture.settle();
    let mut broken = Vec::new();
    let mut compare = |step: &str, fixture: &dyn WindowFixture| {
        let state = window.state();
        broken.extend(disagreements(step, &state, fixture.readback()));
    };
    compare("at open", fixture);
    for command in sample_commands(&window.state()) {
        if window.admit_and_apply(command.clone()).is_err() {
            continue;
        }
        fixture.settle();
        compare(&format!("after {command:?}"), fixture);
    }
    finish(broken)
}

/// A command admitted against the capabilities the window had before the
/// window system revised them is answered `Superseded` by the window, which
/// checks the token against what its snapshot publishes now, and changes
/// nothing.
fn a_command_admitted_before_a_revision_is_superseded(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    let window = fixture.window();
    let title = WindowCommand::SetTitle("admitted before the revision".to_owned());
    let Ok(early) = title.clone().admit(&window.state().capabilities) else {
        return Ok(());
    };
    if !fixture.revise_capabilities() {
        return Ok(());
    }
    let before = window.state();
    match window.apply(early) {
        Err(ApplyError::Superseded { command }) if command == title => {}
        other => {
            return Err(format!(
                "a title admitted before the capabilities changed answered {other:?}, not \
                 Superseded"
            ));
        }
    }
    fixture.settle();
    if window.state().title != before.title {
        return Err("a superseded title reached the window".to_owned());
    }
    Ok(())
}

/// The verbs a window takes outside the command queue — the frame-rate hint
/// and the input regions — answer `Unsupported` exactly when
/// their capability is `Unavailable`, never as a refusal worth trying again,
/// and are taken otherwise (ADR-0254 §3).
fn per_frame_verbs_report_unsupported_exactly_when_unavailable(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    let window = fixture.window();
    let capabilities = window.state().capabilities;
    let mut broken = Vec::new();
    let mut judge =
        |verb: String, availability: Availability, answer: Result<(), Unsupported>| match (
            availability.is_available(),
            answer,
        ) {
            (false, Err(refused)) if refused.is_retryable() => broken.push(format!(
                "{verb} is Unavailable but was refused as {refused}, which asks to be tried again"
            )),
            (true, Ok(())) | (false, Err(_)) => {}
            (true, Err(refused)) => {
                broken.push(format!("{verb} is {availability:?} but answered {refused}"));
            }
            (false, Ok(())) => broken.push(format!("{verb} is Unavailable but answered Ok")),
        };
    for hint in [FrameRateHint::High, FrameRateHint::Default] {
        judge(
            format!("set_frame_rate_hint({hint:?})"),
            capabilities.frame_rate_hint,
            window.set_frame_rate_hint(hint),
        );
    }
    let region = InputRegion::new(
        InputRegionKind::SystemGestureExclusion,
        Rect::from_ltrb(0.0, 0.0, 40.0, 30.0),
    );
    let input_regions = fixture.input_regions();
    for regions in [&[region][..], &[]] {
        judge(
            format!("set_input_regions({} regions)", regions.len()),
            input_regions,
            window.set_input_regions(regions),
        );
    }
    finish(broken)
}

/// Close the window by the command where the window takes it, the way the
/// application closes a window; else the way the window system would. A
/// window already closed (by an earlier check) is left as it is.
fn close_window(fixture: &mut dyn WindowFixture, broken: &mut Vec<String>) {
    let window = fixture.window();
    let state = window.state();
    let probe = WindowCommand::SetTitle(state.title.clone().unwrap_or_default());
    if let Ok(admitted) = probe.admit(&state.capabilities)
        && window.apply(admitted) == Err(ApplyError::Closed)
    {
        return;
    }
    if state.capabilities.permits(&WindowCommand::Close) {
        if let Err(refusal) = window.admit_and_apply(WindowCommand::Close) {
            broken.push(format!("Close is supported but answered {refusal:?}"));
        }
        fixture.settle();
    } else {
        fixture.close();
    }
}

/// `a_surface_target_is_send_sync_and_unavailable_after_close` (ADR-0254 §5):
/// the target crosses threads, hands out a handle while a window with a
/// native surface is open, returns from `pre_present_notify` at once, and
/// answers `Unavailable` once the window closed.
fn a_surface_target_is_send_sync_and_unavailable_after_close(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};

    const fn send_sync<T: Send + Sync + 'static>(_: &T) {}

    let window = fixture.window();
    let target = window.surface_target();
    send_sync(&target);
    let mut broken = Vec::new();
    match (target.window_handle(), fixture.has_native_surface()) {
        (Ok(_), true) | (Err(HandleError::Unavailable), false) => {}
        (answer, native) => broken.push(format!(
            "an open window {} a native surface answered {:?}",
            if native { "with" } else { "without" },
            answer.map(|_| ())
        )),
    }
    // The raster side calls it on its own thread before each present: it
    // returns there, and at once.
    let (done, returned) = std::sync::mpsc::channel();
    let presenting = std::sync::Arc::clone(&target);
    std::thread::spawn(move || {
        presenting.pre_present_notify();
        let _ = done.send(());
    });
    if returned
        .recv_timeout(std::time::Duration::from_secs(1))
        .is_err()
    {
        broken.push("pre_present_notify did not return".to_owned());
    }
    close_window(fixture, &mut broken);
    if !matches!(target.window_handle(), Err(HandleError::Unavailable)) {
        broken.push("a closed window's target still hands out a window handle".to_owned());
    }
    if !matches!(target.display_handle(), Err(HandleError::Unavailable))
        && fixture.has_native_surface()
    {
        broken.push("a closed window's target still hands out a display handle".to_owned());
    }
    finish(broken)
}

fn a_closed_window_refuses(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let mut broken = Vec::new();
    close_window(fixture, &mut broken);
    for command in sample_commands(&window.state()) {
        // What the closed window's last snapshot admits still reaches it, as
        // a command queued before the close does.
        let Ok(admitted) = command.clone().admit(&window.state().capabilities) else {
            continue;
        };
        match window.apply(admitted) {
            Err(ApplyError::Closed) => {}
            other => broken.push(format!("{command:?} after close answered {other:?}")),
        }
    }
    finish(broken)
}

/// Pixels the window system may round a requested number by.
fn near(actual: f64, wanted: f64) -> bool {
    (actual - wanted).abs() <= 1.0
}

fn finish(broken: Vec<String>) -> Result<(), String> {
    if broken.is_empty() {
        Ok(())
    } else {
        Err(broken.join("; "))
    }
}
