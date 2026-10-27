//! The window conformance checks against the headless backend under several
//! capability sets, and against fixtures with one fault each: the checks must
//! pass the first and name the check that catches every fault. Checks that
//! cannot fail prove nothing.

use std::rc::Rc;
use std::sync::Arc;

use flui_native::{HeadlessPlatform, HostWindow, MockWindow, Platform};
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    AdmittedCommand, ApplyError, CommandRefusal, CursorError, CursorIcon, PlatformHaptics,
    PlatformSystemChrome, PlatformTextInput, PlatformWindow, WindowCommand, WindowId, WindowMode,
    WindowState,
};
use flui_platform::{Availability, WindowCapabilities, WindowOptions};
use flui_testing::window_conformance::{self, Readback, WindowFixture};

/// A headless window with a fault of its own: its snapshot claims every
/// capability while the window takes none ([`Fault::ClaimsWhatItCannot`]),
/// it stays open when told to close ([`Fault::IgnoresClose`]), or it
/// publishes a snapshot without the facts the window system reports
/// ([`Fault::SnapshotWithoutFacts`]).
struct Faulty(Rc<dyn PlatformWindow>, Fault);

impl PlatformWindow for Faulty {
    fn id(&self) -> WindowId {
        self.0.id()
    }
    fn request_redraw(&self) {
        self.0.request_redraw();
    }
    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        self.0.redraw_requester()
    }
    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        self.0.surface_target()
    }
    fn state(&self) -> WindowState {
        let mut state = self.0.state();
        match self.1 {
            // The snapshot of a window that keeps its metrics to itself.
            Fault::SnapshotWithoutFacts => state.metrics = None,
            Fault::ClaimsWhatItCannot => state.capabilities = WindowCapabilities::ALL,
            _ => {}
        }
        state
    }
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if command.command() == &WindowCommand::Close && matches!(self.1, Fault::IgnoresClose) {
            return Ok(());
        }
        if matches!(self.1, Fault::IgnoresTheToken) {
            // A host that checks the command against what it honours now,
            // not against the capabilities the token was admitted under.
            let command = command.command().clone();
            return match command.admit_and_apply(&*self.0) {
                Ok(()) => Ok(()),
                Err(CommandRefusal::Apply(error)) => Err(error),
                Err(CommandRefusal::Admission(_)) => Err(ApplyError::failed(
                    "the window does not honour it now".to_owned(),
                )),
            };
        }
        self.0.apply(command)
    }
    fn set_frame_rate_hint(
        &self,
        hint: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        match self.1 {
            Fault::VerbsIgnoreTheirCapabilities => Ok(()),
            _ => self.0.set_frame_rate_hint(hint),
        }
    }

    fn set_input_regions(
        &self,
        regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        match self.1 {
            Fault::VerbsIgnoreTheirCapabilities => Ok(()),
            _ => self.0.set_input_regions(regions),
        }
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
}

/// A headless window under `capabilities`, with `fault` applied to how the
/// fixture settles or closes it.
struct Fixture {
    window: Rc<dyn PlatformWindow>,
    /// The headless window itself, whose model is the window system.
    model: Rc<dyn HostWindow>,
    fault: Option<Fault>,
}

impl Fixture {
    fn mock(&self) -> &MockWindow {
        self.model
            .as_any()
            .downcast_ref::<MockWindow>()
            .expect("a headless platform opens mock windows")
    }
}

#[derive(Clone, Copy)]
enum Fault {
    /// The window system reverts every accepted command while settling.
    RevertsWhileSettling,
    /// The window stays open when asked to close.
    IgnoresClose,
    /// The window's snapshot claims capabilities the window does not have.
    ClaimsWhatItCannot,
    /// The window applies a command it honours now, whatever capabilities the
    /// token was admitted under.
    IgnoresTheToken,
    /// The window's snapshot holds no metrics.
    SnapshotWithoutFacts,
    /// The window's verbs take everything, whatever its capabilities say: the
    /// inert default every verb had before it answered `Unsupported`.
    VerbsIgnoreTheirCapabilities,
}

impl Fixture {
    fn new(capabilities: WindowCapabilities, fault: Option<Fault>) -> Self {
        Self::with_input(capabilities, None, fault)
    }

    /// A headless window under `capabilities` whose platform delivers
    /// `input` in place of its default input availability.
    fn with_input(
        capabilities: WindowCapabilities,
        input: Option<InputCapabilities>,
        fault: Option<Fault>,
    ) -> Self {
        let mut platform = HeadlessPlatform::new().with_window_capabilities(capabilities);
        if let Some(input) = input {
            platform = platform.with_input_availability(input);
        }
        let model = platform
            .open_window(
                WindowOptions::default(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a headless window opens");
        let window: Rc<dyn PlatformWindow> = match fault {
            Some(
                fault @ (Fault::ClaimsWhatItCannot
                | Fault::SnapshotWithoutFacts
                | Fault::IgnoresClose
                | Fault::IgnoresTheToken
                | Fault::VerbsIgnoreTheirCapabilities),
            ) => Rc::new(Faulty(Rc::clone(&model) as _, fault)),
            _ => Rc::clone(&model) as _,
        };
        Self {
            window,
            model,
            fault,
        }
    }
}

impl WindowFixture for Fixture {
    fn window(&self) -> Rc<dyn PlatformWindow> {
        Rc::clone(&self.window)
    }

    /// The headless host's model: what its window system says, read apart
    /// from the snapshot.
    fn readback(&self) -> Readback {
        let system = self.mock().window_system();
        Readback {
            metrics: system.metrics,
            focused: system.focused,
            // The headless desktop's activation is its focus.
            active: system.focused,
            visible: system.visible,
            material: Some(system.material),
        }
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
                // The faulty window system reverts what it can, while the
                // window is open; what it cannot revert stays.
                match command.admit_and_apply(&*self.window) {
                    Ok(())
                    | Err(
                        CommandRefusal::Admission(_) | CommandRefusal::Apply(ApplyError::Closed),
                    ) => {}
                    Err(error) => panic!("the headless window failed a revert: {error}"),
                }
            }
        }
    }

    /// The user's close, through the simulated window system.
    fn close(&mut self) {
        if !matches!(self.fault, Some(Fault::IgnoresClose)) {
            self.mock().simulate_close();
        }
    }

    /// The simulated window system stops reporting where the window opens
    /// again, and keeps the rest.
    fn revise_capabilities(&mut self) -> bool {
        let mut revised = self.model.state().capabilities;
        revised.places_at_open = match revised.places_at_open {
            flui_platform::Availability::Unavailable => flui_platform::Availability::Native,
            _ => flui_platform::Availability::Unavailable,
        };
        self.mock().simulate_capabilities(revised);
        true
    }

    fn input_regions(&self) -> Availability {
        self.mock().window_system().input_regions
    }
}

fn only_the_move_and_size_commands() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_position = flui_platform::Availability::Native;
    capabilities.sets_size = flui_platform::Availability::Native;
    capabilities.observes_position = flui_platform::Availability::Native;
    capabilities
}

fn no_fullscreen_or_minimize() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::ALL;
    capabilities.enters_fullscreen = flui_platform::Availability::Unavailable;
    capabilities.minimizes = flui_platform::Availability::Unavailable;
    capabilities.observes_minimized = flui_platform::Availability::Unavailable;
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
    let without_input_regions = InputCapabilities::builder()
        .pen(Availability::Native)
        .build();
    let violations = window_conformance::check(&mut Fixture::with_input(
        WindowCapabilities::ALL,
        Some(without_input_regions),
        None,
    ));
    assert!(violations.is_empty(), "no input regions: {violations:?}");
}

fn a_window_whose_verbs_ignore_their_capabilities_is_caught() {
    for capabilities in [WindowCapabilities::NONE, WindowCapabilities::ALL] {
        let without_input_regions = InputCapabilities::builder().build();
        let checks: Vec<_> = window_conformance::check(&mut Fixture::with_input(
            capabilities,
            Some(without_input_regions),
            Some(Fault::VerbsIgnoreTheirCapabilities),
        ))
        .into_iter()
        .map(|violation| violation.check)
        .collect();
        assert_eq!(
            checks,
            ["per_frame_verbs_report_unsupported_exactly_when_unavailable"]
        );
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

fn a_window_that_claims_what_it_cannot_do_is_caught() {
    let checks: Vec<_> = window_conformance::check(&mut Fixture::new(
        WindowCapabilities::NONE,
        Some(Fault::ClaimsWhatItCannot),
    ))
    .into_iter()
    .map(|violation| violation.check)
    .collect();
    // The checks that send a claimed command see it fail too; the honesty
    // check names the cause first.
    assert_eq!(
        checks.first(),
        Some(&"capabilities_are_honest"),
        "caught by {checks:?}"
    );
}

fn a_window_that_ignores_the_token_is_caught() {
    let checks = caught(Fault::IgnoresTheToken);
    assert_eq!(
        checks,
        ["a_command_admitted_before_a_revision_is_superseded"]
    );
}

fn a_snapshot_without_the_windows_facts_is_caught() {
    let checks = caught(Fault::SnapshotWithoutFacts);
    assert_eq!(checks, ["the_snapshot_agrees_with_the_window_system"]);
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
                "a_window_that_claims_what_it_cannot_do_is_caught",
                a_window_that_claims_what_it_cannot_do_is_caught as fn(),
            ),
            (
                "a_snapshot_without_the_windows_facts_is_caught",
                a_snapshot_without_the_windows_facts_is_caught as fn(),
            ),
            (
                "a_window_that_ignores_the_token_is_caught",
                a_window_that_ignores_the_token_is_caught as fn(),
            ),
            (
                "a_window_whose_verbs_ignore_their_capabilities_is_caught",
                a_window_whose_verbs_ignore_their_capabilities_is_caught as fn(),
            ),
        ],
    );
}
