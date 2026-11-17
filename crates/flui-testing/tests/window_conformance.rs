//! The window conformance checks against the headless backend under several
//! capability sets, and against fixtures with one fault each: the checks must
//! pass the first and name the check that catches every fault. Checks that
//! cannot fail prove nothing.

use std::rc::Rc;
use std::sync::Arc;

use flui_native::{HeadlessHostWindow, HeadlessPlatform, HostWindow, Platform};
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    AdmittedCommand, ApplyError, CommandRefusal, CursorError, CursorIcon, PlatformSystemChrome,
    PlatformTextInput, PlatformWindow, WindowCommand, WindowFrame, WindowId, WindowMode,
    WindowState, haptics::PlatformHaptics,
};
use flui_platform::{
    Availability, EventTime, FrameRequest, WindowCapabilities, WindowCapability, WindowOptions,
};
use flui_testing::window_conformance::{self, Readback, WindowFixture};
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

/// A headless window with a fault of its own: its snapshot claims every
/// capability while the window takes none ([`Fault::ClaimsWhatItCannot`]),
/// it stays open when told to close ([`Fault::IgnoresClose`]), or it
/// publishes a snapshot without the facts the window system reports
/// ([`Fault::SnapshotWithoutFacts`]). It reports the id it is given, which is
/// the window's own unless the fault is [`Fault::ReissuesAClosedId`].
struct Faulty(
    Rc<dyn PlatformWindow>,
    Fault,
    WindowId,
    KeptSnapshot,
    std::cell::RefCell<Option<Claimed>>,
);

/// What a window with a presentation fault publishes as its own, for one set
/// of capabilities its window system holds.
#[derive(Debug, Clone, Copy)]
struct Claimed {
    /// The window system's capabilities, as the host last read them.
    read: WindowCapabilities,
    /// Its claim over them, published once.
    published: WindowCapabilities,
}

/// The snapshot a window with [`Fault::DropsFactsWithABadReport`] keeps
/// answering while its window system reports no window: set when the report
/// comes, cleared by the next resize.
type KeptSnapshot = Rc<std::cell::RefCell<Option<WindowState>>>;

impl Faulty {
    /// `window` with `fault`, reporting its own id.
    fn new(window: Rc<dyn PlatformWindow>, fault: Fault, kept: KeptSnapshot) -> Self {
        let id = window.id();
        Self(window, fault, id, kept, std::cell::RefCell::default())
    }

    /// What the window publishes over the window system's `held`: `held`
    /// itself, or, for a presentation fault, its claim, published again only
    /// when what the window system holds changes, as a host publishes.
    fn published(&self, held: WindowCapabilities) -> WindowCapabilities {
        let claim = match self.1 {
            Fault::ClaimsPresentation => WindowCapability::Native,
            Fault::DisclaimsPresentation => WindowCapability::NOT_ON_THIS_PLATFORM,
            _ => return held,
        };
        let mut claimed = self.4.borrow_mut();
        if let Some(Claimed { read, published }) = *claimed
            && read.revision() == held.revision()
            && read == held
        {
            return published;
        }
        let published = presentation_claimed(held, claim)
            .publish()
            .expect("the process has publications left");
        *claimed = Some(Claimed {
            read: held,
            published,
        });
        published
    }
}

impl PlatformWindow for Faulty {
    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        Ok(None)
    }

    fn window_at(&self, _: flui_foundation::geometry::Point<f64>) -> flui_platform::WindowAt {
        flui_platform::WindowAt::Unknown
    }

    fn reveal_after_first_frame(&self) {}

    fn id(&self) -> WindowId {
        self.2
    }
    fn request_redraw(&self) {
        self.0.request_redraw();
    }
    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        self.0.redraw_requester()
    }
    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        match self.1 {
            Fault::PresentNotifyBlocks => Arc::new(BlockingSurface),
            _ => self.0.surface_target(),
        }
    }
    fn state(&self) -> WindowState {
        if let Some(kept) = self.3.borrow().clone() {
            // A host that drops a whole sample for one bad value: nothing
            // else the window system says reaches the snapshot.
            return kept;
        }
        let mut state = self.0.state();
        match self.1 {
            // The snapshot of a window that keeps its metrics to itself.
            Fault::SnapshotWithoutFacts => state.metrics = None,
            Fault::ClaimsWhatItCannot => state.capabilities = WindowCapabilities::ALL,
            Fault::HidesWhenMinimized => state.visible &= state.mode != WindowMode::Minimized,
            Fault::RepublishesOnEveryRead => {
                state.capabilities = state
                    .capabilities
                    .publish()
                    .expect("the process has publications left");
            }
            Fault::ClaimsPresentation | Fault::DisclaimsPresentation => {
                state.capabilities = self.published(state.capabilities);
            }
            _ => {}
        }
        state
    }
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if matches!(command.command(), WindowCommand::SetSize(_)) {
            // A resize is a report that describes a window again.
            self.3.borrow_mut().take();
        }
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
        if matches!(
            self.1,
            Fault::ClaimsPresentation | Fault::DisclaimsPresentation
        ) {
            // The host opens the token under what it published, and hands the
            // window system the command under what that holds: the claim is
            // about presentation, which admits no command.
            let held = self.0.state().capabilities;
            let command = command.against(&self.published(held))?;
            let admitted = command
                .admit(&held)
                .expect("the claim names no capability a command is admitted against");
            return self.0.apply(admitted);
        }
        if matches!(self.1, Fault::RepublishesOnEveryRead) {
            // The host opens the token under what it reads now, a publication
            // of its own every time.
            let command = command.against(&self.state().capabilities)?;
            let admitted = command
                .admit(&self.0.state().capabilities)
                .expect("the republished capabilities are the window system's");
            return self.0.apply(admitted);
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
        match (self.1, self.0.set_input_regions(regions)) {
            (Fault::VerbsIgnoreTheirCapabilities, _) => Ok(()),
            // A window that keeps the regions of a window that is gone and
            // says it took them.
            (Fault::TakesInputRegionsAfterClose, Err(gone))
                if gone.reason == flui_platform::UnsupportedReason::NoWindow =>
            {
                Ok(())
            }
            (_, answer) => answer,
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

/// A surface target whose `pre_present_notify` does not return in time.
struct BlockingSurface;

impl raw_window_handle::HasWindowHandle for BlockingSurface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl raw_window_handle::HasDisplayHandle for BlockingSurface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl flui_platform::SurfaceTarget for BlockingSurface {
    fn pre_present_notify(
        &self,
        _: Option<flui_foundation::FrameStamp>,
        _: flui_platform::SwapChainStatistics,
    ) {
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    fn keep_past_close(&self) {}
}

/// A headless window under `capabilities`, with `fault` applied to how the
/// fixture settles or closes it.
struct Fixture {
    /// The platform that opened the window, which opens the next one.
    platform: HeadlessPlatform,
    window: Rc<dyn PlatformWindow>,
    /// The headless window itself, whose model is the window system.
    model: Rc<dyn HostWindow>,
    /// The handler the window was opened with, which hears its frames.
    handler: Rc<RecordingWindowHandler>,
    fault: Option<Fault>,
    /// What a window with [`Fault::DropsFactsWithABadReport`] keeps answering.
    kept: KeptSnapshot,
    /// How many presents the fixture made into the window.
    presents: u64,
}

impl Fixture {
    fn mock(&self) -> &HeadlessHostWindow {
        self.model
            .as_any()
            .downcast_ref::<HeadlessHostWindow>()
            .expect("a headless platform opens headless windows")
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
    /// The window's surface target blocks its `pre_present_notify`.
    PresentNotifyBlocks,
    /// The fixture says the window has a native surface its target never
    /// hands out.
    SurfaceWithheld,
    /// The window's snapshot says it reports presentation, and the window
    /// system behind it reports none.
    ClaimsPresentation,
    /// The window's snapshot says it reports no presentation, and its frame
    /// requests carry reports.
    DisclaimsPresentation,
    /// The window reads a minimized window as hidden, and so does the
    /// fixture's own readback of it, the way X11's map state does.
    HidesWhenMinimized,
    /// The window's frame requests carry the platform's stamps on the
    /// platform's own clock, never rebased onto the event epoch.
    StampsFramesOffTheTimeline,
    /// The window's presentation reports name a frame no present carried, as
    /// a host that names a report after what it holds now rather than the
    /// present the report describes.
    NamesAnotherFrame,
    /// The window opened after the one under test closed reports the closed
    /// one's id, as a host that took a recycled handle for an id would.
    ReissuesAClosedId,
    /// The window publishes a report whose numbers describe no window as an
    /// empty window, the way a host that reads a size it cannot build as
    /// zero (`unwrap_or_default`) would.
    PublishesAReportOfNoWindow,
    /// While its window system reports numbers that describe no window, the
    /// window publishes nothing else either: one bad value costs the whole
    /// sample, its title, focus and close among them.
    DropsFactsWithABadReport,
    /// The window answers `Ok` to the input regions of a window that closed.
    TakesInputRegionsAfterClose,
    /// The host publishes the window's capabilities anew on every read, so a
    /// command admitted against its snapshot never opens.
    RepublishesOnEveryRead,
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
        let handler = RecordingWindowHandler::new();
        let model = platform
            .open_window(WindowOptions::default(), Rc::clone(&handler) as _)
            .expect("a headless window opens");
        let kept = KeptSnapshot::default();
        let window: Rc<dyn PlatformWindow> = match fault {
            Some(
                fault @ (Fault::ClaimsWhatItCannot
                | Fault::SnapshotWithoutFacts
                | Fault::IgnoresClose
                | Fault::IgnoresTheToken
                | Fault::VerbsIgnoreTheirCapabilities
                | Fault::TakesInputRegionsAfterClose
                | Fault::PresentNotifyBlocks
                | Fault::ClaimsPresentation
                | Fault::DisclaimsPresentation
                | Fault::HidesWhenMinimized
                | Fault::DropsFactsWithABadReport
                | Fault::RepublishesOnEveryRead),
            ) => Rc::new(Faulty::new(Rc::clone(&model) as _, fault, Rc::clone(&kept))),
            _ => Rc::clone(&model) as _,
        };
        Self {
            platform,
            window,
            model,
            handler,
            fault,
            kept,
            presents: 0,
        }
    }
}

impl WindowFixture for Fixture {
    /// A second window of the same platform, with the faulty id where the
    /// fault reissues the closed one's.
    fn open_another(&mut self) -> Option<Rc<dyn PlatformWindow>> {
        let another = self
            .platform
            .open_window(WindowOptions::default(), RecordingWindowHandler::new())
            .expect("a headless window opens");
        Some(match self.fault {
            Some(fault @ Fault::ReissuesAClosedId) => Rc::new(Faulty(
                another as _,
                fault,
                self.window.id(),
                KeptSnapshot::default(),
                std::cell::RefCell::default(),
            )),
            _ => another as _,
        })
    }

    fn window(&self) -> Rc<dyn PlatformWindow> {
        Rc::clone(&self.window)
    }

    /// The headless host's model: what its window system says, read apart
    /// from the snapshot.
    fn readback(&self) -> Readback {
        let system = self.mock().window_system();
        let minimized = self.model.state().mode == WindowMode::Minimized;
        Readback {
            metrics: system.metrics,
            focused: system.focused,
            // The headless desktop's activation is its focus.
            active: system.focused,
            visible: system.visible
                && !(minimized && matches!(self.fault, Some(Fault::HidesWhenMinimized))),
            occluded: Some(system.occluded),
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
                WindowCommand::SetFrame(WindowFrame::System),
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
        let mut revised = self.model.state().capabilities.into_parts();
        revised.places_at_open = match revised.places_at_open {
            flui_platform::Availability::Unavailable => flui_platform::Availability::Native,
            _ => flui_platform::Availability::Unavailable,
        };
        self.mock()
            .simulate_capabilities(revised.into_capabilities());
        true
    }

    /// The simulated window system reports a size that is no extent, which
    /// the headless window refuses as a host refuses a rectangle over a
    /// scale of zero.
    fn report_no_window(&mut self) -> bool {
        match self.fault {
            Some(Fault::PublishesAReportOfNoWindow) => {
                // What a host that reads the size it cannot build as zero
                // publishes for the report.
                let mut collapsed = self.model.state();
                collapsed.outer = flui_platform::WindowGeometry::EMPTY;
                collapsed.content_size = flui_platform::WindowExtent::ZERO;
                self.mock().simulate_state(collapsed);
            }
            Some(Fault::DropsFactsWithABadReport) => {
                *self.kept.borrow_mut() = Some(self.window.state());
                self.mock().simulate_resize(f64::NAN, f64::INFINITY);
            }
            _ => self.mock().simulate_resize(f64::NAN, f64::INFINITY),
        }
        true
    }

    fn input_capabilities(&self) -> flui_platform::input_capabilities::InputCapabilities {
        self.mock().window_system().input_capabilities
    }

    /// A headless window has no native surface.
    fn has_native_surface(&self) -> bool {
        matches!(self.fault, Some(Fault::SurfaceWithheld))
    }

    /// One frame presented through the window's own target, which the
    /// simulated window system shows at once and reports where the window
    /// says it reports presentation; then the window's next frame.
    fn present_and_request_frame(&mut self) -> Option<FrameRequest> {
        self.presents += 1;
        self.model.surface_target().pre_present_notify(
            Some(window_conformance::fixture_frame(self.presents)),
            flui_platform::SwapChainStatistics::None,
        );
        let _shown = self.mock().simulate_presented(EventTime::now(), None);
        self.handler.take_calls();
        self.window.request_redraw();
        let request = self
            .handler
            .take_calls()
            .into_iter()
            .rev()
            .find_map(|call| match call {
                HandlerCall::FrameRequested(request) => Some(request),
                _ => None,
            });
        match self.fault {
            // An hour on, a counter whose epoch is not the process's, and a
            // target present that now lies before it.
            Some(Fault::StampsFramesOffTheTimeline) => request.map(|mut request| {
                let platform_clock = request.vsync + std::time::Duration::from_secs(3600);
                request.target_present = Some(request.vsync);
                request.vsync = platform_clock;
                request
            }),
            // A frame of another presentation than the fixture presents.
            Some(Fault::NamesAnotherFrame) => request.map(|mut request| {
                if let Some(presented) = request.presented.as_mut() {
                    presented.frame.address.presentation_id =
                        flui_foundation::PresentationId::new(2);
                }
                request
            }),
            _ => request,
        }
    }
}

/// `capabilities` with the presentation report a faulty host claims, which
/// are no publication of the window's.
fn presentation_claimed(
    capabilities: WindowCapabilities,
    claimed: WindowCapability,
) -> WindowCapabilities {
    let mut parts = capabilities.into_parts();
    parts.reports_presentation = claimed;
    parts.into_capabilities()
}

fn only_the_move_and_size_commands() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE.into_parts();
    capabilities.sets_position = flui_platform::WindowCapability::Native;
    capabilities.sets_size = flui_platform::WindowCapability::Native;
    capabilities.observes_position = flui_platform::Availability::Native;
    capabilities.into_capabilities()
}

fn no_fullscreen_or_minimize() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::ALL.into_parts();
    capabilities.enters_fullscreen = flui_platform::WindowCapability::NOT_ON_THIS_PLATFORM;
    capabilities.minimizes = flui_platform::WindowCapability::NOT_ON_THIS_PLATFORM;
    capabilities.observes_minimized = flui_platform::Availability::Unavailable;
    capabilities.into_capabilities()
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
        // A window that takes every verb takes the input regions of a closed
        // window too.
        assert_eq!(
            checks,
            [
                "per_frame_verbs_report_unsupported_exactly_when_unavailable",
                "a_closed_window_refuses"
            ]
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

fn a_host_that_reissues_a_closed_windows_id_is_caught() {
    assert_eq!(
        caught(Fault::ReissuesAClosedId),
        ["a_window_opened_after_a_close_has_an_id_of_its_own"]
    );
}

fn a_window_that_publishes_a_report_of_no_window_is_caught() {
    assert_eq!(
        caught(Fault::PublishesAReportOfNoWindow),
        ["a_report_that_describes_no_window_publishes_nothing"]
    );
}

fn a_window_that_drops_its_facts_with_a_bad_report_is_caught() {
    assert_eq!(
        caught(Fault::DropsFactsWithABadReport),
        ["a_report_that_describes_no_window_publishes_nothing"]
    );
}

fn a_closed_window_that_takes_input_regions_is_caught() {
    assert_eq!(
        caught(Fault::TakesInputRegionsAfterClose),
        ["a_closed_window_refuses"]
    );
}

fn a_window_that_ignores_the_token_is_caught() {
    let checks = caught(Fault::IgnoresTheToken);
    assert_eq!(
        checks,
        [
            "a_command_admitted_before_a_revision_is_superseded",
            "a_window_opens_only_its_own_publication"
        ]
    );
}

/// A host that publishes anew on every read never opens a command admitted
/// against its own snapshot.
fn a_window_that_republishes_on_every_read_is_caught() {
    let checks = caught(Fault::RepublishesOnEveryRead);
    assert!(
        checks.contains(&"a_window_opens_only_its_own_publication"),
        "caught by {checks:?}"
    );
}

fn a_surface_target_that_blocks_or_withholds_is_caught() {
    for fault in [Fault::PresentNotifyBlocks, Fault::SurfaceWithheld] {
        assert_eq!(
            caught(fault),
            ["a_surface_target_is_send_sync_and_unavailable_after_close"]
        );
    }
}

fn a_window_whose_presentation_reports_disagree_with_its_capabilities_is_caught() {
    assert_eq!(
        window_conformance::check(&mut Fixture::new(
            WindowCapabilities::NONE,
            Some(Fault::ClaimsPresentation),
        ))
        .into_iter()
        .map(|violation| violation.check)
        .collect::<Vec<_>>(),
        ["presentation_is_reported_exactly_when_claimed"]
    );
    assert_eq!(
        caught(Fault::DisclaimsPresentation),
        ["presentation_is_reported_exactly_when_claimed"]
    );
}

/// A report that names a frame no present carried is caught, however well its
/// time fits.
fn a_window_whose_reports_name_another_frame_is_caught() {
    assert_eq!(
        caught(Fault::NamesAnotherFrame),
        ["presentation_is_reported_exactly_when_claimed"]
    );
}

fn a_window_whose_frame_stamps_are_off_the_timeline_is_caught() {
    assert_eq!(
        caught(Fault::StampsFramesOffTheTimeline),
        ["a_frame_request_is_stamped_on_the_event_timeline"]
    );
}

fn a_snapshot_without_the_windows_facts_is_caught() {
    let checks = caught(Fault::SnapshotWithoutFacts);
    assert_eq!(checks, ["the_snapshot_agrees_with_the_window_system"]);
}

/// A window that hides itself when minimized is caught even where its
/// fixture's readback agrees with it: the expectation that a minimized window
/// stays shown is the contract's, not the window system's.
fn a_window_hidden_by_minimizing_is_caught_whatever_its_readback_says() {
    let checks = caught(Fault::HidesWhenMinimized);
    assert_eq!(checks, ["accepted_commands_converge"]);
}

/// The snapshot's occlusion is the window system's, and is compared only
/// while the window is shown and not minimized.
fn the_snapshot_follows_the_window_systems_occlusion() {
    let mut fixture = Fixture::new(WindowCapabilities::ALL, None);
    fixture.mock().simulate_occlusion(true);
    let state = fixture.window.state();
    assert!(
        state.visible && state.occluded && !state.is_seen(),
        "{state:?}"
    );
    assert_eq!(window_conformance::check(&mut fixture), []);
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
                "a_host_that_reissues_a_closed_windows_id_is_caught",
                a_host_that_reissues_a_closed_windows_id_is_caught as fn(),
            ),
            (
                "a_snapshot_without_the_windows_facts_is_caught",
                a_snapshot_without_the_windows_facts_is_caught as fn(),
            ),
            (
                "a_window_hidden_by_minimizing_is_caught_whatever_its_readback_says",
                a_window_hidden_by_minimizing_is_caught_whatever_its_readback_says as fn(),
            ),
            (
                "the_snapshot_follows_the_window_systems_occlusion",
                the_snapshot_follows_the_window_systems_occlusion as fn(),
            ),
            (
                "a_window_that_ignores_the_token_is_caught",
                a_window_that_ignores_the_token_is_caught as fn(),
            ),
            (
                "a_window_that_republishes_on_every_read_is_caught",
                a_window_that_republishes_on_every_read_is_caught as fn(),
            ),
            (
                "a_window_that_publishes_a_report_of_no_window_is_caught",
                a_window_that_publishes_a_report_of_no_window_is_caught as fn(),
            ),
            (
                "a_window_that_drops_its_facts_with_a_bad_report_is_caught",
                a_window_that_drops_its_facts_with_a_bad_report_is_caught as fn(),
            ),
            (
                "a_closed_window_that_takes_input_regions_is_caught",
                a_closed_window_that_takes_input_regions_is_caught as fn(),
            ),
            (
                "a_surface_target_that_blocks_or_withholds_is_caught",
                a_surface_target_that_blocks_or_withholds_is_caught as fn(),
            ),
            (
                "a_window_whose_verbs_ignore_their_capabilities_is_caught",
                a_window_whose_verbs_ignore_their_capabilities_is_caught as fn(),
            ),
            (
                "a_window_whose_presentation_reports_disagree_with_its_capabilities_is_caught",
                a_window_whose_presentation_reports_disagree_with_its_capabilities_is_caught
                    as fn(),
            ),
            (
                "a_window_whose_reports_name_another_frame_is_caught",
                a_window_whose_reports_name_another_frame_is_caught as fn(),
            ),
            (
                "a_window_whose_frame_stamps_are_off_the_timeline_is_caught",
                a_window_whose_frame_stamps_are_off_the_timeline_is_caught as fn(),
            ),
        ],
    );
}
