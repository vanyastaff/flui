//! What every window backend promises about its state and commands
//! (ADR-0237), checked from outside.
//!
//! A backend runs [`assert_conforms`] against a [`WindowFixture`] of its window.
//! The checks drive the window only through [`PlatformWindow`], the way the
//! application's window handle does, so a backend cannot pass by being tested
//! against its own assumptions:
//!
//! - **Capabilities are honest.** A command is refused as
//!   [`Unsupported`](WindowCommandError::Unsupported) exactly when its
//!   capability is `false`: never accepted and ignored, never refused while
//!   claimed, and refused for good — its reason is not one worth asking again
//!   ([`Unsupported::is_retryable`](flui_platform::Unsupported::is_retryable)).
//! - **State converges.** A command the window accepted shows in its
//!   [`state`](PlatformWindow::state) once the window system has settled: the
//!   title, size, position, mode, visibility, flags and constraints it asked for.
//! - **Revisions mean change.** Reading the state twice with nothing between
//!   keeps its revision; a command that changed something raises it.
//! - **Bad numbers never reach the window system.** A size that is not finite
//!   and positive is refused as
//!   [`InvalidGeometry`](WindowCommandError::InvalidGeometry).
//! - **A closed window refuses.** Every command after the window closed is
//!   [`Closed`](WindowCommandError::Closed).
//! - **The snapshot agrees with the window system.** Once the window system
//!   has settled, each fact the fixture reads back by its own means (the
//!   drawable metrics and scale, focus, visibility) is the snapshot's, and the
//!   snapshot's capabilities are the window's (ADR-0254 §1).
//!
//! [`check`] returns each violated promise by name; [`assert_conforms`] panics
//! listing them.

use std::sync::Arc;

use flui_foundation::geometry::{Point, Size, WindowMetrics};
use flui_platform::{
    PlatformWindow, SizeConstraints, WindowCommand, WindowCommandError, WindowMode, WindowState,
};

/// What the window system says of the window now, read by the fixture by its
/// own means (the operating system's queries, a headless host's model), never
/// through the window's snapshot. `None` for a fact the fixture cannot read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Readback {
    /// The drawable area in device pixels and its scale; `None` also while the
    /// window has no drawable area (minimized), whose snapshot keeps the last.
    pub metrics: Option<WindowMetrics>,
    /// Whether the window has input focus.
    pub focused: Option<bool>,
    /// Whether the window is shown and nothing (a cloak, a locked session)
    /// hides it.
    pub visible: Option<bool>,
}

/// A window under test, as the checks drive it.
pub trait WindowFixture {
    /// The window under test, open and visible, in normal mode.
    fn window(&self) -> Arc<dyn PlatformWindow>;

    /// What the window system says of the window now.
    fn readback(&self) -> Readback;

    /// Let the window system carry out what was accepted: pump the event loop
    /// until it is quiet. A window that applies commands synchronously keeps
    /// this empty.
    fn settle(&mut self);

    /// Close the window the way the window system would, and settle.
    fn close(&mut self);
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
    ]
}

fn capabilities_are_honest(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let mut broken = Vec::new();
    for command in sample_commands(&window.state()) {
        let capable = window.capabilities().permits(&command);
        let answer = window.apply(command.clone());
        fixture.settle();
        match (capable, &answer) {
            (false, Err(WindowCommandError::Unsupported(refused))) if refused.is_retryable() => {
                broken.push(format!(
                    "{command:?} is not supported but was refused as {refused}, which asks to be \
                     tried again"
                ));
            }
            (false, Err(WindowCommandError::Unsupported(_))) | (true, Ok(())) => {}
            (false, other) => broken.push(format!(
                "{command:?} is not supported but answered {other:?}, not Unsupported"
            )),
            (true, Err(WindowCommandError::Unsupported(_))) => {
                broken.push(format!("{command:?} is supported but was refused"));
            }
            (true, Err(other)) => {
                broken.push(format!("{command:?} is supported but answered {other:?}"));
            }
        }
    }
    finish(broken)
}

fn accepted_commands_converge(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let capabilities = window.capabilities();
    let mut broken = Vec::new();
    for command in sample_commands(&window.state()) {
        if !capabilities.permits(&command) || window.apply(command.clone()).is_err() {
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
    if !window.capabilities().permits(&command) {
        return Ok(());
    }
    window
        .apply(command)
        .map_err(|error| format!("SetTitle was refused: {error}"))?;
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
    let capabilities = window.capabilities();
    let mut broken = Vec::new();
    for command in [
        WindowCommand::SetSize(Size::new(f64::NAN, 100.0)),
        WindowCommand::SetSize(Size::new(-5.0, 100.0)),
        WindowCommand::SetPosition(Point::new(f64::INFINITY, 0.0)),
    ] {
        if !capabilities.permits(&command) {
            continue;
        }
        let before = window.state();
        match window.apply(command.clone()) {
            Err(WindowCommandError::InvalidGeometry) => {}
            other => broken.push(format!("{command:?} answered {other:?}")),
        }
        fixture.settle();
        if window.state() != before {
            broken.push(format!("{command:?} changed the window"));
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
    if let Some(focused) = readback.focused
        && state.focused != focused
    {
        broken.push(format!(
            "{step}: the snapshot says focused {}, the window system {focused}",
            state.focused
        ));
    }
    if let Some(visible) = readback.visible
        && state.visible != visible
    {
        broken.push(format!(
            "{step}: the snapshot says visible {}, the window system {visible}",
            state.visible
        ));
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
        if state.capabilities != window.capabilities() {
            broken.push(format!(
                "{step}: the snapshot's capabilities are {:?}, the window's {:?}",
                state.capabilities,
                window.capabilities()
            ));
        }
    };
    compare("at open", fixture);
    for command in sample_commands(&window.state()) {
        if !window.capabilities().permits(&command) || window.apply(command.clone()).is_err() {
            continue;
        }
        fixture.settle();
        compare(&format!("after {command:?}"), fixture);
    }
    finish(broken)
}

fn a_closed_window_refuses(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    fixture.close();
    let mut broken = Vec::new();
    for command in sample_commands(&window.state()) {
        if !window.capabilities().permits(&command) {
            continue;
        }
        match window.apply(command.clone()) {
            Err(WindowCommandError::Closed) => {}
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
