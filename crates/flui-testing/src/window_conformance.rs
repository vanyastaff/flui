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
//!   is `Absent`, for good — its reason is not one worth asking again
//!   ([`Unsupported::is_retryable`](flui_platform::Unsupported::is_retryable))
//!   — and the window applies every command its capabilities admitted: never
//!   claimed and then refused.
//! - **State converges.** A command the window accepted shows in its
//!   [`state`](PlatformWindow::state) once the window system has settled: the
//!   title, size, position, mode, visibility, flags and constraints it asked for,
//!   and the window system draws the material it asked for. A minimized
//!   window is still shown: minimizing is a mode, not a hide, whatever the
//!   window system's own reading of "visible" says.
//! - **Revisions mean change.** Reading the state twice with nothing between
//!   keeps its revision; a command that changed something raises it.
//! - **Bad numbers never reach the window system.** A size that is not finite
//!   and positive is refused at admission as
//!   [`InvalidGeometry`](AdmissionError::InvalidGeometry), so no window sees it.
//! - **Nor does a bad report reach the snapshot.** A report whose numbers
//!   describe no window (a size that is not finite, a scale of zero)
//!   publishes nothing of its geometry, and reading the window again keeps
//!   its revision; the window's other facts are still published while it
//!   stands (ADR-0254 §1). A fixture with no way to make its window system
//!   report one ([`WindowFixture::report_no_window`]) skips it.
//! - **Verbs answer to their capability.** The frame-rate hint and the input
//!   regions, which a window takes outside the command queue because they are
//!   sent every frame, answer `Unsupported` exactly when their capability is
//!   `Unavailable` (ADR-0254 §3). The material is a command like the others.
//! - **Presentation is reported exactly when claimed.** A window whose
//!   capabilities say it reports when its frames reached the screen carries
//!   the report on its first frame request after frames were shown, stamped
//!   between the first present and the request's arrival; one that says it
//!   does not carries none. A fixture that cannot present
//!   ([`WindowFixture::present_and_request_frame`]) skips it.
//! - **Frame requests are on the event timeline.** A frame request's vsync
//!   lies within a second before its arrival, never after it, and the
//!   platform's target present time, where it names one, within a second
//!   after the vsync: a platform stamp the backend did not rebase onto the
//!   event epoch lands far from both.
//! - **A closed window refuses.** Every admitted command after the window
//!   closed — through [`WindowCommand::Close`] where the window takes it, else
//!   the way the window system closes it — is [`Closed`](ApplyError::Closed),
//!   and the input regions, which a window takes outside the command queue,
//!   are refused: a window that took them while it was open answers
//!   [`NoWindow`](UnsupportedReason::NoWindow).
//! - **The snapshot agrees with the window system.** Once the window system
//!   has settled, each fact the fixture reads back by its own means (the
//!   drawable metrics and scale, focus, visibility, occlusion where it can be
//!   asked) is the snapshot's
//!   (ADR-0254 §1). The capabilities the checks read are the snapshot's.
//! - **A grab ends with the focus.** A focused window that takes a grab
//!   publishes it and its window system holds it
//!   ([`WindowFixture::held_grab`]); once the fixture takes the focus away
//!   ([`WindowFixture::take_focus_away`]) the window system holds none and
//!   the snapshot says so.
//! - **A window opens only its own publication** (ADR-0341). A command
//!   admitted against the window's snapshot opens, so a window that publishes
//!   anew on every read is caught; one admitted against capabilities the caller
//!   published itself, or against another window's, answers
//!   [`Superseded`](ApplyError::Superseded).
//! - **A window's id is its own.** A window the host opens after the one
//!   under test closed has another [`WindowId`](flui_platform::WindowId),
//!   whatever handle the window system reuses for it (ADR-0039 §4). A
//!   fixture that cannot open a second window
//!   ([`WindowFixture::open_another`]) skips it.
//!
//! [`check`] returns each violated promise by name; [`assert_conforms`] panics
//! listing them.

use std::rc::Rc;

use flui_foundation::geometry::{Point, Rect, Size, WindowMetrics};
use flui_platform::input::input_regions::{InputRegion, InputRegionKind};
use flui_platform::{
    AdmissionError, AppTitleBar, ApplyError, AspectRatio, Availability, CursorGrab, EventTime,
    FrameRateHint, FrameRequest, PlatformWindow, SizeConstraints, Unsupported, UnsupportedReason,
    WindowCapability, WindowCommand, WindowExtent, WindowFocus, WindowFrame, WindowLevel,
    WindowMaterial, WindowMode, WindowPosition, WindowState,
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
    /// Whether the window is shown: Win32's `IsWindowVisible`, an X11 window
    /// mapped or iconic, AppKit's `isVisible` or `isMiniaturized`. Minimizing
    /// and covering leave it shown.
    pub visible: bool,
    /// Whether the window system says none of the window reaches the screen
    /// (a cloak, a covering, a suspended toplevel), as it says it; `None`
    /// where it answers no query for it (X11 reports occlusion only
    /// as an event). The checks compare it with the snapshot only for a
    /// shown window in another mode than minimized, which is when the
    /// snapshot keeps it.
    pub occluded: Option<bool>,
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

    /// Make the window system report, for the window, numbers that describe
    /// no window (a size that is not finite, a scale of zero), and settle;
    /// `false` where the window system gives a test no such lever, which
    /// skips the check that needs one.
    fn report_no_window(&mut self) -> bool {
        false
    }

    /// The input regions the window's input path honours, as its
    /// [`InputCapabilities`](flui_platform::input::input_capabilities::InputCapabilities)
    /// report them: the capabilities the input-region verb answers to, which
    /// the snapshot does not carry.
    fn input_capabilities(&self) -> flui_platform::input::input_capabilities::InputCapabilities;

    /// Whether the window has a native surface, which its surface target
    /// hands out a window handle for while it is open, on the thread the
    /// checks run on (ADR-0254 §5); a headless window has none.
    fn has_native_surface(&self) -> bool;

    /// Present frames into a window's surface the way a renderer does
    /// (the surface target's
    /// [`pre_present_notify`](flui_platform::SurfaceTarget::pre_present_notify)
    /// with the present's frame, [`fixture_frame`] of its count from 1, and
    /// what the presenter read, then the present), let the window system
    /// show them, then have the window ask for a frame and return soon after
    /// the request arrived, not after a settle: the checks time its stamps
    /// against their reading as this returns, and a quiet second waited out
    /// after it would age them past the slack. The frame request its handler
    /// received. The window is the fixture's where the
    /// window system still shows it whatever the earlier checks did, else one
    /// of the same host the fixture opens for this and closes after (a
    /// minimized Wayland window shows no frame and cannot be restored). `None` where the fixture cannot present
    /// into the window, which leaves the presentation check to the
    /// capability's own honesty.
    fn present_and_request_frame(&mut self) -> Option<FrameRequest> {
        None
    }

    /// Open another window of the same host the way the window under test
    /// opened, once that window closed, and settle. `None` where the fixture
    /// cannot open a second window, which skips the identity check.
    fn open_another(&mut self) -> Option<Rc<dyn PlatformWindow>> {
        None
    }

    /// The grab the window system holds for the window, read by the
    /// fixture's own means (Win32's cursor clip, a headless host's model),
    /// never through the snapshot; `None` where it answers no such query,
    /// which leaves the grab check to the snapshot alone.
    fn held_grab(&self) -> Option<CursorGrab> {
        None
    }

    /// Take the keyboard focus away from the window the way the user
    /// switching to another window does, and settle; `false` where the
    /// window system gives a test no such lever, which skips the check that
    /// a grab ends with the focus.
    fn take_focus_away(&mut self) -> bool {
        false
    }
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
        "a_report_that_describes_no_window_publishes_nothing",
        a_report_that_describes_no_window_publishes_nothing,
    ),
    (
        "the_snapshot_agrees_with_the_window_system",
        the_snapshot_agrees_with_the_window_system,
    ),
    (
        "a_command_admitted_before_a_revision_is_superseded",
        a_command_admitted_before_a_revision_is_superseded,
    ),
    ("a_grab_ends_with_the_focus", a_grab_ends_with_the_focus),
    (
        "a_window_opens_only_its_own_publication",
        a_window_opens_only_its_own_publication,
    ),
    (
        "per_frame_verbs_report_unsupported_exactly_when_unavailable",
        per_frame_verbs_report_unsupported_exactly_when_unavailable,
    ),
    (
        "presentation_is_reported_exactly_when_claimed",
        presentation_is_reported_exactly_when_claimed,
    ),
    (
        "a_frame_request_is_stamped_on_the_event_timeline",
        a_frame_request_is_stamped_on_the_event_timeline,
    ),
    (
        "a_surface_target_is_send_sync_and_unavailable_after_close",
        a_surface_target_is_send_sync_and_unavailable_after_close,
    ),
    ("a_closed_window_refuses", a_closed_window_refuses),
    (
        "a_window_opened_after_a_close_has_an_id_of_its_own",
        a_window_opened_after_a_close_has_an_id_of_its_own,
    ),
];

/// Every command kind, with numbers a window of any size can take, built from
/// the window's current state so toggles actually toggle. A position is on
/// the display the window is on: a window on no display has no place to be
/// moved to, and its list has no position.
fn sample_commands(state: &WindowState) -> Vec<WindowCommand> {
    let position = state.display.map(|display| {
        WindowCommand::SetPosition(
            WindowPosition::try_new(display, Point::new(120.0, 90.0))
                .expect("BUG: the sample position is finite"),
        )
    });
    let mut commands = vec![
        WindowCommand::SetTitle("window conformance".to_owned()),
        WindowCommand::SetSize(extent(640.0, 480.0)),
        WindowCommand::SetConstraints(
            SizeConstraints::try_new(
                Some(Size::new(200.0, 150.0)),
                Some(Size::new(2000.0, 1500.0)),
            )
            .expect("BUG: the sample limits are finite"),
        ),
        // The frame first: a window system that resizes a window only through
        // its frame (Win32) takes resizing back only on a framed window.
        WindowCommand::SetFrame(WindowFrame::AppTitleBar(AppTitleBar::new())),
        WindowCommand::SetFrame(if state.frame == WindowFrame::System {
            WindowFrame::None
        } else {
            WindowFrame::System
        }),
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
        WindowCommand::SetLevel(WindowLevel::AlwaysOnTop),
        WindowCommand::SetLevel(WindowLevel::AlwaysOnBottom),
        WindowCommand::SetLevel(WindowLevel::Normal),
        WindowCommand::SetResizeIncrements(Some(
            WindowExtent::try_new(Size::new(8.0, 16.0)).expect("BUG: the sample step is finite"),
        )),
        WindowCommand::SetResizeIncrements(None),
        WindowCommand::SetAspectRatio(Some(AspectRatio::new(
            std::num::NonZeroU32::MIN.saturating_add(3),
            std::num::NonZeroU32::MIN.saturating_add(2),
        ))),
        WindowCommand::SetAspectRatio(None),
    ];
    commands.splice(1..1, position);
    commands
}

/// The extent `width` by `height`, finite by the kit's own numbers.
fn extent(width: f64, height: f64) -> WindowExtent {
    WindowExtent::try_new(Size::new(width, height)).expect("BUG: the kit's sizes are finite")
}

/// A window holds the pointer only while it has the keyboard focus: a grab
/// it takes is published and held by the window system, and once the focus
/// goes the window system holds none and the snapshot says so. A window that
/// takes no grab, cannot be focused here, or whose fixture cannot take the
/// focus away, skips the part it cannot show.
fn a_grab_ends_with_the_focus(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let capabilities = window.state().capabilities;
    let grab = if capabilities.confines_cursor().is_available() {
        CursorGrab::Confined
    } else if capabilities.locks_cursor().is_available() {
        CursorGrab::Locked
    } else {
        return Ok(());
    };
    let _activated = window.admit_and_apply(WindowCommand::Activate);
    fixture.settle();
    if !window.state().focus.is_focused() {
        return Ok(());
    }
    let mut broken = Vec::new();
    if let Err(refused) = window.admit_and_apply(WindowCommand::SetCursorGrab(grab)) {
        return Err(format!("{grab:?} is supported but was refused: {refused}"));
    }
    fixture.settle();
    let held = window.state().cursor_grab;
    if held != grab {
        broken.push(format!(
            "a focused window asked for {grab:?} publishes {held:?}"
        ));
    }
    if let Some(system) = fixture.held_grab()
        && system != grab
    {
        broken.push(format!(
            "a focused window asked for {grab:?}: the window system holds {system:?}"
        ));
    }
    if fixture.take_focus_away() {
        let state = window.state();
        if state.focus.is_focused() {
            broken.push("the window kept the focus the fixture took away".to_owned());
        }
        if state.cursor_grab != CursorGrab::None {
            broken.push(format!(
                "after the focus went, the snapshot still says {:?}",
                state.cursor_grab
            ));
        }
        if let Some(system) = fixture.held_grab()
            && system != CursorGrab::None
        {
            broken.push(format!(
                "after the focus went, the window system holds {system:?}"
            ));
        }
    } else if let Err(refused) =
        window.admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::None))
    {
        broken.push(format!("letting the grab go was refused: {refused}"));
    }
    fixture.settle();
    finish(broken)
}

fn capabilities_are_honest(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let mut broken = Vec::new();
    for command in sample_commands(&window.state()) {
        let capabilities = window.state().capabilities;
        let capable = capabilities.permits(&command);
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
                if capabilities.observes_position().is_available() {
                    // The same display, and the same point of it.
                    let there = state.outer.position();
                    there
                        .is_some_and(|there| {
                            there.display() == at.display()
                                && near(there.at().x, at.at().x)
                                && near(there.at().y, at.at().y)
                        })
                        .then_some(())
                        .ok_or_else(|| format!("the window is at {there:?}, not {at:?}"))
                } else {
                    Ok(())
                }
            }
            WindowCommand::SetSize(size) => (near(state.content_size.width(), size.width())
                && near(state.content_size.height(), size.height()))
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
            WindowCommand::SetFrame(frame) => (state.frame == *frame)
                .then_some(())
                .ok_or_else(|| format!("the frame is {:?}, not {frame:?}", state.frame)),
            // Minimizing is a mode, not a hide: the window stays shown, and
            // `is_seen` is what leaves it out.
            WindowCommand::SetMode(WindowMode::Minimized) => {
                if !state.visible {
                    Err("the window is no longer shown".to_owned())
                } else if capabilities.observes_minimized().is_available() {
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
            WindowCommand::SetLevel(level) => (state.level == *level)
                .then_some(())
                .ok_or_else(|| format!("the level is {:?}, not {level:?}", state.level)),
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
    // A size or position that is not finite cannot be put in a command; what
    // is left to refuse is a size, a limit or a step that encloses no area.
    for command in [
        WindowCommand::SetSize(extent(0.0, 100.0)),
        WindowCommand::SetSize(extent(640.0, 0.0)),
        WindowCommand::SetResizeIncrements(Some(extent(0.0, 8.0))),
    ] {
        // Before the capabilities: no window, whatever it honours, is handed
        // numbers that describe none.
        match command.clone().admit(&window.state().capabilities) {
            Err(AdmissionError::InvalidGeometry(_)) => {}
            other => broken.push(format!("{command:?} was admitted as {other:?}")),
        }
    }
    finish(broken)
}

/// `a_report_that_describes_no_window_publishes_nothing` (ADR-0254 §1): a
/// report whose numbers no window has (a size that is not finite, a scale of
/// zero) is no state change. The snapshot stays the one published, at its
/// revision, however often it is read: a snapshot that held such a number
/// would differ from itself and publish a revision with every read. The
/// report costs the window its geometry and nothing else: a title it takes
/// while the report stands is published. The window then follows its window
/// system again: the next size it takes is
/// published.
fn a_report_that_describes_no_window_publishes_nothing(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    let window = fixture.window();
    fixture.settle();
    let before = window.state();
    if !fixture.report_no_window() {
        return Ok(());
    }
    let mut broken = Vec::new();
    let after = window.state();
    if after != before {
        broken.push(format!(
            "a report that describes no window published {after:?} over {before:?}"
        ));
    }
    let again = window.state();
    if again.revision != after.revision {
        broken.push(format!(
            "reading again moved the revision from {:?} to {:?}",
            after.revision, again.revision
        ));
    }
    // The bad report costs the window its geometry and nothing else: a fact
    // the window takes while its window system still reports it is
    // published.
    let title = "a title the report does not hide";
    if window
        .admit_and_apply(WindowCommand::SetTitle(title.to_owned()))
        .is_ok()
    {
        fixture.settle();
        let titled = window.state();
        if titled.title.as_deref() != Some(title) {
            broken.push(format!(
                "with the report standing, the title it took was not published: {:?}",
                titled.title
            ));
        }
    }
    let size = extent(600.0, 400.0);
    if window.admit_and_apply(WindowCommand::SetSize(size)).is_ok() {
        fixture.settle();
        let recovered = window.state();
        if !(near(recovered.content_size.width(), size.width())
            && near(recovered.content_size.height(), size.height()))
        {
            broken.push(format!(
                "after the report, a resize to {size:?} left the content at {:?}",
                recovered.content_size
            ));
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
    let occlusion = readback
        .occluded
        .map(|read| read && readback.visible && state.mode != WindowMode::Minimized)
        .map(|read| ("occluded", state.occluded, read));
    // The window system's two reports, read as one standing by the one rule
    // every host follows; a hidden window stands nowhere.
    let focus = if readback.visible {
        WindowFocus::reported(readback.focused, readback.active)
    } else {
        WindowFocus::Inactive
    };
    if state.focus != focus {
        broken.push(format!(
            "{step}: the snapshot says {:?}, the window system {focus:?}",
            state.focus
        ));
    }
    for (fact, published, read) in [("visible", state.visible, readback.visible)]
        .into_iter()
        .chain(occlusion)
    {
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
            capabilities.frame_rate_hint(),
            window.set_frame_rate_hint(hint),
        );
    }
    let region = InputRegion::new(
        InputRegionKind::SystemGestureExclusion,
        Rect::from_ltrb(0.0, 0.0, 40.0, 30.0),
    );
    let input_regions = if fixture.input_capabilities().honours_input_regions() {
        Availability::Native
    } else {
        Availability::Unavailable
    };
    for regions in [&[region][..], &[]] {
        judge(
            format!("set_input_regions({} regions)", regions.len()),
            input_regions,
            window.set_input_regions(regions),
        );
    }
    finish(broken)
}

/// The frame a fixture's `present`-th present into a window carries, counted
/// from 1, as the raster side stamps it
/// ([`SurfaceTarget::pre_present_notify`](flui_platform::SurfaceTarget::pre_present_notify)):
/// frame `present` of one presentation, on its first surface. The presentation
/// check expects a report to name one of these frames.
#[must_use]
pub fn fixture_frame(present: u64) -> flui_foundation::FrameStamp {
    flui_foundation::FrameStamp::new(
        fixture_presentation(),
        flui_foundation::FrameEpoch::from_raw(present),
        flui_foundation::SurfaceGeneration::ZERO.next(),
    )
}

/// The presentation whose frames a fixture presents.
fn fixture_presentation() -> flui_foundation::PresentationAddress {
    flui_foundation::PresentationAddress {
        ui_runtime_id: flui_foundation::UiRuntimeId::new(1),
        presentation_id: flui_foundation::PresentationId::new(1),
    }
}

/// `presentation_is_reported_exactly_when_claimed`: a window whose
/// capabilities say it reports when its frames reached the screen carries a
/// report on the first frame request after frames were shown, naming one of
/// the frames the fixture presented ([`fixture_frame`]) and stamped on the
/// event timeline between the first present and the time the request arrived;
/// a window that says it does not answers why with a lasting reason and
/// carries none.
fn presentation_is_reported_exactly_when_claimed(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    // An absent capability names a lasting reason by its type
    // (`CapabilityAbsence`): there is no answer that asks to be tried again.
    let claimed = fixture.window().state().capabilities.reports_presentation();
    let mut broken = Vec::new();
    let before = EventTime::now();
    let Some(request) = fixture.present_and_request_frame() else {
        return Ok(());
    };
    let after = EventTime::now();
    match (claimed, request.presented) {
        (WindowCapability::Native | WindowCapability::Emulated, Some(presented))
            if presented.frame.address != fixture_presentation()
                || presented.frame.epoch == flui_foundation::FrameEpoch::ZERO =>
        {
            broken.push(format!(
                "a report names {:?}, no frame the fixture presented",
                presented.frame
            ));
        }
        (WindowCapability::Native | WindowCapability::Emulated, Some(presented))
            if presented.at < before || presented.at > after =>
        {
            broken.push(format!(
                "a frame shown after {before:?} was reported at {:?}, outside the time up to its \
                 request's arrival at {after:?}",
                presented.at
            ));
        }
        (WindowCapability::Native | WindowCapability::Emulated, Some(_))
        | (WindowCapability::Absent(_), None) => {}
        (WindowCapability::Native | WindowCapability::Emulated, None) => broken.push(format!(
            "presentation feedback is {claimed:?}, but the request after shown frames carried no \
             report"
        )),
        (WindowCapability::Absent(absence), Some(presented)) => broken.push(format!(
            "presentation feedback is absent as {absence:?}, but a request carried a report at \
             {:?}",
            presented.at
        )),
    }
    finish(broken)
}

/// `a_frame_request_is_stamped_on_the_event_timeline`: the vsync of the
/// frame request after shown frames lies within [`FRAME_STAMP_SLACK`] before
/// the request's arrival and never after it, and a target present time within
/// that slack after the vsync. A refresh period is far shorter than the
/// slack, so a correctly rebased stamp always fits; a platform reading left on
/// its own clock (a raw performance counter, a compositor's epoch) does not.
fn a_frame_request_is_stamped_on_the_event_timeline(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    let Some(request) = fixture.present_and_request_frame() else {
        return Ok(());
    };
    let arrived = EventTime::now();
    let mut broken = Vec::new();
    if request.vsync > arrived
        || arrived.saturating_duration_since(request.vsync) > FRAME_STAMP_SLACK
    {
        broken.push(format!(
            "a frame request that arrived by {arrived:?} has its vsync at {:?}",
            request.vsync
        ));
    }
    if let Some(target) = request.target_present
        && (target <= request.vsync
            || target.saturating_duration_since(request.vsync) > FRAME_STAMP_SLACK)
    {
        broken.push(format!(
            "a frame request whose vsync is {:?} targets its present at {target:?}",
            request.vsync
        ));
    }
    finish(broken)
}

/// How far a frame request's stamps may lie from its arrival and from each
/// other: longer than any refresh period and any wait between a vsync and its
/// request, shorter than any clock mix-up.
const FRAME_STAMP_SLACK: std::time::Duration = std::time::Duration::from_secs(1);

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
        presenting.pre_present_notify(None, flui_platform::SwapChainStatistics::None);
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

/// A window opens a command only under its own publication (ADR-0341): the
/// normal mode, which every window takes, admitted against the window's
/// snapshot opens; admitted against the same capabilities published by the
/// caller, it answers `Superseded`. A window that publishes anew on every read
/// fails the first half.
fn a_window_opens_only_its_own_publication(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let normal = WindowCommand::SetMode(WindowMode::Normal);
    let mut broken = Vec::new();
    if let Ok(published_by_the_caller) = window.state().capabilities.publish() {
        match normal.clone().admit(&published_by_the_caller) {
            Ok(foreign) => match window.apply(foreign) {
                Err(ApplyError::Superseded { .. }) => {}
                other => broken.push(format!(
                    "the normal mode admitted against capabilities the caller published \
                     answered {other:?}, not Superseded"
                )),
            },
            Err(refused) => broken.push(format!(
                "capabilities the caller published refused the normal mode: {refused}"
            )),
        }
    }
    match normal.admit(&window.state().capabilities) {
        Ok(own) => {
            if let Err(error) = window.apply(own) {
                broken.push(format!(
                    "the normal mode admitted against the window's snapshot answered {error:?}"
                ));
            }
        }
        Err(refused) => broken.push(format!(
            "the window's snapshot refused the normal mode: {refused}"
        )),
    }
    fixture.settle();
    finish(broken)
}

fn a_closed_window_refuses(fixture: &mut dyn WindowFixture) -> Result<(), String> {
    let window = fixture.window();
    let honoured_input_regions = fixture.input_capabilities().honours_input_regions();
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
    // The verb a window takes outside the command queue has no `Closed` to
    // answer: a window that took input regions while it was open says its
    // window is not there, and never that it kept them.
    let region = InputRegion::new(
        InputRegionKind::SystemGestureExclusion,
        Rect::from_ltrb(0.0, 0.0, 40.0, 30.0),
    );
    match window.set_input_regions(&[region]) {
        Err(refused)
            if !honoured_input_regions || refused.reason == UnsupportedReason::NoWindow => {}
        answer => broken.push(format!(
            "set_input_regions after close answered {answer:?}, not that the window is not there"
        )),
    }
    finish(broken)
}

/// `a_window_opened_after_a_close_has_an_id_of_its_own` (ADR-0039 §4): the
/// host issues the next window another id, even where the operating system
/// hands that window the closed one's handle or address, so nothing addressed
/// to the closed window reaches it, and a command admitted against the closed
/// window's publication answers `Superseded` there (ADR-0341).
fn a_window_opened_after_a_close_has_an_id_of_its_own(
    fixture: &mut dyn WindowFixture,
) -> Result<(), String> {
    let closed = fixture.window().id();
    let closed_capabilities = fixture.window().state().capabilities;
    let mut broken = Vec::new();
    close_window(fixture, &mut broken);
    if let Some(another) = fixture.open_another() {
        let reopened = another.id();
        if reopened == closed {
            broken.push(format!(
                "a window opened after {closed:?} closed has its id"
            ));
        }
        // Another window's publication opens nothing here (ADR-0341).
        let normal = WindowCommand::SetMode(WindowMode::Normal);
        if let Ok(foreign) = normal.admit(&closed_capabilities) {
            match another.apply(foreign) {
                Err(ApplyError::Superseded { .. }) => {}
                other => broken.push(format!(
                    "a command admitted against the closed window's capabilities answered \
                     {other:?} on the window opened after it, not Superseded"
                )),
            }
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
