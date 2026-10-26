//! The window conformance checks against the headless backend under several
//! capability sets, and against fixtures with one fault each: the checks must
//! pass the first and name the check that catches every fault. Checks that
//! cannot fail prove nothing.

use std::sync::Arc;

use flui_foundation::geometry::{DeviceSize, Size};
use flui_native::{HeadlessPlatform, Platform};
use flui_platform::{
    CursorError, CursorIcon, PlatformHaptics, PlatformSystemChrome, PlatformTextInput,
    PlatformWindow, Unsupported, UnsupportedReason, WindowCommand, WindowCommandError, WindowId,
    WindowMode, WindowState,
};
use flui_platform::{WindowCapabilities, WindowOptions};
use flui_testing::window_conformance::{self, WindowFixture};

/// A window that refuses what it does not take as unavailable for now, as if
/// asking again could succeed.
struct RefusesForNow(Arc<dyn PlatformWindow>);

impl PlatformWindow for RefusesForNow {
    fn id(&self) -> WindowId {
        self.0.id()
    }
    fn physical_size(&self) -> DeviceSize {
        self.0.physical_size()
    }
    fn logical_size(&self) -> Size {
        self.0.logical_size()
    }
    fn scale_factor(&self) -> f64 {
        self.0.scale_factor()
    }
    fn request_redraw(&self) {
        self.0.request_redraw();
    }
    fn is_focused(&self) -> bool {
        self.0.is_focused()
    }
    fn is_visible(&self) -> bool {
        self.0.is_visible()
    }
    fn state(&self) -> WindowState {
        self.0.state()
    }
    fn capabilities(&self) -> WindowCapabilities {
        self.0.capabilities()
    }
    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        self.0.apply(command).map_err(|error| match error {
            WindowCommandError::Unsupported(_) => {
                Unsupported::new(UnsupportedReason::Unavailable).into()
            }
            other => other,
        })
    }
    fn close(&self) {
        self.0.close();
    }
    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.0.set_cursor(cursor)
    }
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.0.text_input()
    }
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        self.0.haptics()
    }
    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        self.0.system_chrome()
    }
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        self.0.drag_source()
    }
    fn show(&self) -> Result<(), flui_platform::WindowShowError> {
        self.0.show()
    }
}

/// A headless window under `capabilities`, with `fault` applied to how the
/// fixture settles or closes it.
struct Fixture {
    window: Arc<dyn PlatformWindow>,
    fault: Option<Fault>,
}

#[derive(Clone, Copy)]
enum Fault {
    /// The window system reverts every accepted command while settling.
    RevertsWhileSettling,
    /// The window stays open when asked to close.
    IgnoresClose,
    /// The window refuses what it does not take as unavailable for now.
    RefusesForNow,
}

impl Fixture {
    fn new(capabilities: WindowCapabilities, fault: Option<Fault>) -> Self {
        let platform = HeadlessPlatform::new().with_window_capabilities(capabilities);
        let window: Arc<dyn PlatformWindow> = platform
            .open_window(WindowOptions::default())
            .expect("a headless window opens");
        let window = if matches!(fault, Some(Fault::RefusesForNow)) {
            Arc::new(RefusesForNow(window))
        } else {
            window
        };
        Self { window, fault }
    }
}

impl WindowFixture for Fixture {
    fn window(&self) -> Arc<dyn PlatformWindow> {
        Arc::clone(&self.window)
    }

    fn settle(&mut self) {
        if matches!(self.fault, Some(Fault::RevertsWhileSettling)) {
            for command in [
                WindowCommand::SetMode(WindowMode::Normal),
                WindowCommand::SetTitle("reverted".to_owned()),
                WindowCommand::Show,
                WindowCommand::SetResizable(true),
                WindowCommand::SetDecorated(true),
            ] {
                let _ = self.window.apply(command);
            }
        }
    }

    fn close(&mut self) {
        if !matches!(self.fault, Some(Fault::IgnoresClose)) {
            self.window.close();
        }
    }
}

fn only_the_move_and_size_commands() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_position = true;
    capabilities.sets_size = true;
    capabilities.observes_position = true;
    capabilities
}

fn no_fullscreen_or_minimize() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::ALL;
    capabilities.enters_fullscreen = false;
    capabilities.minimizes = false;
    capabilities.observes_minimized = false;
    capabilities
}

fn the_headless_window_conforms_under_every_capability_set() {
    for (name, capabilities) in [
        ("all", WindowCapabilities::ALL),
        ("none", WindowCapabilities::NONE),
        ("move and size only", only_the_move_and_size_commands()),
        ("no fullscreen or minimize", no_fullscreen_or_minimize()),
    ] {
        let violations = window_conformance::check(&mut Fixture::new(capabilities, None));
        assert!(violations.is_empty(), "{name}: {violations:?}");
    }
}

fn caught(fault: Fault) -> Vec<&'static str> {
    window_conformance::check(&mut Fixture::new(WindowCapabilities::ALL, Some(fault)))
        .into_iter()
        .map(|violation| violation.check)
        .collect()
}

fn a_window_that_reverts_what_it_accepted_fails_to_converge() {
    let checks = caught(Fault::RevertsWhileSettling);
    assert!(
        checks.contains(&"accepted_commands_converge"),
        "caught by {checks:?}"
    );
}

fn a_window_that_stays_open_is_caught_accepting_commands_after_close() {
    let checks = caught(Fault::IgnoresClose);
    assert_eq!(checks, ["a_closed_window_refuses"]);
}

fn a_window_that_refuses_for_now_what_it_never_takes_is_caught() {
    let checks: Vec<_> = window_conformance::check(&mut Fixture::new(
        WindowCapabilities::NONE,
        Some(Fault::RefusesForNow),
    ))
    .into_iter()
    .map(|violation| violation.check)
    .collect();
    assert_eq!(checks, ["capabilities_are_honest"]);
}

#[test]
fn window_conformance_checks() {
    crate::run_table(
        "window_conformance_checks",
        &[
            (
                "the_headless_window_conforms_under_every_capability_set",
                the_headless_window_conforms_under_every_capability_set as fn(),
            ),
            (
                "a_window_that_reverts_what_it_accepted_fails_to_converge",
                a_window_that_reverts_what_it_accepted_fails_to_converge as fn(),
            ),
            (
                "a_window_that_stays_open_is_caught_accepting_commands_after_close",
                a_window_that_stays_open_is_caught_accepting_commands_after_close as fn(),
            ),
            (
                "a_window_that_refuses_for_now_what_it_never_takes_is_caught",
                a_window_that_refuses_for_now_what_it_never_takes_is_caught as fn(),
            ),
        ],
    );
}
