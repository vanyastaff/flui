//! The window conformance checks against the headless backend under several
//! capability sets, and against fixtures with one fault each: the checks must
//! pass the first and name the check that catches every fault. Checks that
//! cannot fail prove nothing.

use std::sync::Arc;

use flui_native::{HeadlessPlatform, Platform, WindowCapabilities, WindowOptions};
use flui_platform_api::{PlatformWindow, WindowCommand, WindowMode};
use flui_testing::window_conformance::{self, WindowFixture};

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
}

impl Fixture {
    fn new(capabilities: WindowCapabilities, fault: Option<Fault>) -> Self {
        let platform = HeadlessPlatform::new().with_window_capabilities(capabilities);
        let window = platform
            .open_window(WindowOptions::default())
            .expect("a headless window opens");
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
        ],
    );
}
