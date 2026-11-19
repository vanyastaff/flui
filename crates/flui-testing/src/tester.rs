//! [`Tester`]: an application's test, written the way the driver and an agent
//! drive the application.
//!
//! A tester mounts a view as `run_app` does, under the UI runtime's root
//! scopes, in a [`HeadlessHost`] on a manual clock, and reaches it only
//! through the window's development agent ([`AgentWindow`]): it finds
//! elements with `semantics.locate`, checks them with
//! `element.actionability`, and delivers input with `input.inject` through
//! the window's input lease, the synthetic path a driver takes. Nothing here
//! hit-tests, walks a tree or reads a render object itself, so what a test
//! proves is what assistive technology, the driver and an agent see. The one
//! exception is [`Tester::draw_commands`], which reads the scene the host
//! received: the agent's protocol carries no drawing.
//!
//! # Time
//!
//! Only [`Tester::pump`], [`Tester::pump_for`] and
//! [`Tester::pump_and_settle`] move the clock by more than a frame the
//! application asked for. An action returns once its input was delivered and
//! the frame after it ran. An expectation pumps frames while the application
//! has a frame or work due now, and fails at once, listing what the window
//! shows, when the application is idle and the condition is false: a
//! debounce or a timeout of the application never elapses inside an
//! expectation without the test saying so.
//!
//! # Failures
//!
//! Each action and expectation panics at its caller with what it was looking
//! for and what the window showed instead; its `try_` twin returns the same
//! account as a [`TestError`].

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_protocol::method::{
    ElementActionability, InputInject, InputRelease, InspectRender, InspectWidgets,
    SemanticsLocate, SemanticsRead,
};
use flui_protocol::{
    act::Actionability, act::ActionabilityCheck, act::ActionabilityParams, error::ErrorObject,
    geometry::LogicalPoint, geometry::LogicalRect, input::ActionSequence, input::ImeAction,
    input::InputParams, input::InputSource, input::KeyAction, input::PointerAction,
    input::PointerButton, input::PointerDevice, inspect::InspectParams, inspect::InspectReading,
    locator::LocateParams, locator::Locator, locator::Match, session::WindowParams,
    tree::ElementId, tree::Node, tree::ReadParams, tree::Tree, tree::TreeDiagnostic, tree::outline,
    wire::Checked, wire::Role,
};
use flui_runtime::owner_turn::CloseRequestHandler;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};
use flui_view::{CloseReason, View};
use parking_lot::Mutex;

use crate::host::{AppRegistrations, KitStorage};
use crate::storage::{MemoryStorage, RunId};
use crate::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};

/// One frame at 60 Hz: what [`Tester::pump`] advances and what an action or
/// an expectation waits per frame the application asks for.
pub const FRAME: Duration = Duration::from_millis(16);

/// How long an expectation waits by default while the application works.
pub const EXPECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Frames an agent call may take to be answered before the tester reports it
/// unanswered. An injected tap takes three.
const ANSWER_FRAMES: u32 = 64;

/// Outline lines a failure lists.
const SHOWN_LINES: usize = 15;

/// How often [`Tester::try_is_visible`] locates and reads again when a frame
/// changed the tree between the locate and a reading.
const VISIBLE_READS: usize = 4;

/// At most this many zero-length frames run work due now before a read: a
/// woken completion, the rebuild it schedules and the semantics it commits.
const DUE_NOW_FRAMES: u32 = 8;

/// The window a [`Tester`] mounts its view in: its logical size, the
/// platform whose conventions the application follows, the storage its
/// documents are kept in, and the capability providers the application
/// registers.
#[derive(Clone)]
pub struct TestWindow {
    width: u32,
    height: u32,
    platform: Option<flui_platform::TargetPlatform>,
    /// Whether the window plays a browser's page ([`TestWindow::in_browser`]).
    in_browser: bool,
    storage: WindowStorage,
    registrations: AppRegistrations,
    close_request_handler: Option<CloseRequestHandler>,
}

impl std::fmt::Debug for TestWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (overrides, plugins) = self.registrations.counts();
        f.debug_struct("TestWindow")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("platform", &self.platform)
            .field("in_browser", &self.in_browser)
            .field("storage", &self.storage)
            .field("overrides", &overrides)
            .field("plugins", &plugins)
            .field("close_request_handler", &self.close_request_handler)
            .finish()
    }
}

/// Where a [`TestWindow`]'s application keeps its documents.
#[derive(Debug, Clone)]
enum WindowStorage {
    /// A fresh, empty [`MemoryStorage`] for each mount.
    Fresh,
    /// This storage, as left by an earlier run.
    Given(MemoryStorage),
    /// None at all, as on a platform without files.
    Absent,
}

impl TestWindow {
    /// A window of `width` by `height` logical pixels, its application
    /// keeping its documents in a fresh, empty [`MemoryStorage`].
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            platform: None,
            in_browser: false,
            storage: WindowStorage::Fresh,
            registrations: AppRegistrations::new(),
            close_request_handler: None,
        }
    }

    /// Answer the window's close requests with `handler`, as
    /// `AppConfig::close_request_handler` does: asked after the window's close
    /// guard for the user's close and a quit, and first at the session's end,
    /// on the platforms whose window system asks.
    #[must_use]
    pub fn close_request_handler(mut self, handler: CloseRequestHandler) -> Self {
        self.close_request_handler = Some(handler);
        self
    }

    /// Register `plugin`'s providers, as `Application::plugin` does: a widget
    /// acquires what it provides and nothing else does. A
    /// [`restart`](Tester::restart) registers it again.
    ///
    /// The mount panics when two registrations conflict, as a run fails
    /// before its platform starts.
    #[must_use]
    pub fn plugin(mut self, plugin: impl flui_platform::capability::Plugin) -> Self {
        self.registrations.plugin(plugin);
        self
    }

    /// Provide the capability `C` with `provider`, in place of the built-in
    /// one and of any plugin's, as `Application::capability` does.
    ///
    /// The mount panics when two registrations conflict.
    #[must_use]
    pub fn capability<C: flui_platform::capability::Capability>(
        mut self,
        provider: impl flui_platform::capability::CapabilityProvider<C>,
    ) -> Self {
        self.registrations.capability::<C, _>(provider);
        self
    }

    /// Follow `platform`'s conventions (chords, handles, menus, transitions)
    /// instead of the platform-neutral ones a headless window follows.
    #[must_use]
    pub fn platform(mut self, platform: flui_platform::TargetPlatform) -> Self {
        self.platform = Some(platform);
        self
    }

    /// Play a browser's page, on whatever target the test runs
    /// ([`HeadlessWindow::in_browser`]): no close is put to the window's
    /// guard or handler, and the session's end delivers its notice alone.
    #[must_use]
    pub fn in_browser(mut self) -> Self {
        self.in_browser = true;
        self
    }

    /// Keep the application's documents in `storage`: a launch over what an
    /// earlier run left there, or over contents the test put there.
    #[must_use]
    pub fn storage(mut self, storage: MemoryStorage) -> Self {
        self.storage = WindowStorage::Given(storage);
        self
    }

    /// Give the application no storage at all, as a platform without files
    /// runs it: a document it opens is read-only and starts from its initial
    /// value.
    #[must_use]
    pub fn without_storage(mut self) -> Self {
        self.storage = WindowStorage::Absent;
        self
    }
}

impl Default for TestWindow {
    /// 800 by 600 logical pixels, as `flutter_test` sizes its surface.
    fn default() -> Self {
        Self::new(800, 600)
    }
}

/// Why a [`Tester`] action or expectation failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum TestError {
    /// Nothing matches the locator.
    #[error(
        "nothing matches {locator:?}.\nThe window shows:\n{shown}\nA widget that publishes no \
         semantics is found by an identifier on a group around it: wrap it in \
         `Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)` and find it \
         with `find::id`. An identifier alone forms no node a reader sees."
    )]
    NotFound {
        /// What was looked for.
        locator: Box<Locator>,
        /// The window's outline, cut to its first lines.
        shown: String,
    },
    /// Nothing matches the locator, and an identifier it names is on a node
    /// the agent and the accessibility adapters lift into its parent, so
    /// nothing selects it by that identifier (ADR-0240 §3).
    #[error(
        "nothing matches {locator:?}: the identifier {identifier:?} is on a node that publishes \
         nothing of its own (no role, label or text, or a generic or presentation role), which the \
         agent and accessibility clients lift into its parent, so nothing \
         selects it. {fix}\nThe window shows:\n{shown}",
        fix = unpublished_identifier_fix(control.as_deref())
    )]
    UnpublishedIdentifier {
        /// What was looked for.
        locator: Box<Locator>,
        /// The identifier nothing selects.
        identifier: String,
        /// The control the node is lifted into, as its outline line, when it
        /// is one AccessKit names from its contents.
        control: Option<String>,
        /// The window's outline, cut to its first lines.
        shown: String,
    },
    /// The locator matches several elements where one is needed.
    #[error(
        "{count} elements match {locator:?}; narrow it, or pick one with `.pick(Pick::First)` or \
         `Pick::Nth`:\n{candidates}"
    )]
    Ambiguous {
        /// What was looked for.
        locator: Box<Locator>,
        /// How many elements match.
        count: usize,
        /// Their outline lines.
        candidates: String,
    },
    /// The element cannot take the action now.
    #[error("{element} is not {}{covered}", check.name(), covered = covered_by.as_ref().map_or_else(String::new, |by| format!(": the hit-test reaches {by}")))]
    NotActionable {
        /// The element, as its outline line.
        element: String,
        /// The check that does not hold.
        check: ActionabilityCheck,
        /// What the hit-test reached instead.
        covered_by: Option<String>,
    },
    /// An expectation's condition is false and the application has no frame
    /// or work due: waiting would only move the clock.
    #[error(
        "expected {expected} of {locator:?}, found {found}; the application is idle{next}.\nThe \
         window shows:\n{shown}",
        next = next_deadline.map_or_else(String::new, |at| format!(
            " until a deadline {at:?} away (advance the clock with `pump_for` to reach it)"
        ))
    )]
    Idle {
        /// What the expectation asked.
        expected: String,
        /// What was looked for.
        locator: Box<Locator>,
        /// What the window had instead.
        found: String,
        /// The application's next deadline, when it armed one.
        next_deadline: Option<Duration>,
        /// The window's outline, cut to its first lines.
        shown: String,
    },
    /// An expectation's condition stayed false while the application worked
    /// for the whole timeout.
    #[error(
        "expected {expected} of {locator:?} within {timeout:?}, found {found}.\nThe window \
         shows:\n{shown}"
    )]
    Timeout {
        /// What the expectation asked.
        expected: String,
        /// What was looked for.
        locator: Box<Locator>,
        /// What the window had at the end.
        found: String,
        /// How long it waited.
        timeout: Duration,
        /// The window's outline, cut to its first lines.
        shown: String,
    },
    /// The application did not settle within the frames allowed.
    #[error("the application did not settle within {frames} frames")]
    NotSettled {
        /// The frames pumped.
        frames: usize,
    },
    /// The window's agent did not answer within the frames allowed.
    #[error("the window's agent did not answer within {frames} frames")]
    Unanswered {
        /// The frames pumped.
        frames: u32,
    },
    /// The window's agent refused or failed a call.
    #[error("the window's agent answered {}: {}", .0.code, .0.message)]
    Agent(Box<ErrorObject>),
    /// Frames kept changing the window between a locate and the readings of
    /// what it found (its semantics tree, or the scene a drawing is read
    /// from), every time it was read.
    #[error("the window changed between a locate and its readings {reads} times")]
    KeptChanging {
        /// The locates made.
        reads: usize,
    },
    /// The element is an authored semantics region, which no render object
    /// forms, so no drawing is its own.
    #[error(
        "{element} is an authored region no render object forms, so it painted nothing of its \
         own; locate the widget that paints"
    )]
    NoRenderObject {
        /// The element, as its outline line.
        element: String,
    },
    /// A render object below the element has more children than one
    /// inspection lists, so its drawing cannot be read whole.
    #[error(
        "a render object under {element} has more children than one inspection lists \
         ({children} left out); locate a smaller part of it"
    )]
    TooManyChildren {
        /// The element, as its outline line.
        element: String,
        /// How many children the inspection left out.
        children: u32,
    },
    /// A trip to the background or back was asked of a window that names no
    /// phone: only Android and iOS leave the screen that way.
    #[error(
        "{platform:?} is no phone: only a window that names Android or iOS goes to the background"
    )]
    NotAPhone {
        /// The platform the window names.
        platform: flui_platform::TargetPlatform,
    },
    /// A trip to the background was asked while the application is already
    /// there: a phone reports no second trip before a return.
    #[error("the application is already in the background; bring it back with `foreground` first")]
    InBackground,
    /// A return was asked while the application is not in the background.
    #[error("the application is not in the background; send it there with `background` first")]
    InForeground,
}

impl From<ErrorObject> for TestError {
    fn from(error: ErrorObject) -> Self {
        Self::Agent(Box::new(error))
    }
}

/// Keeps the windows the host hands its development agent.
struct Windows(Arc<Mutex<Vec<AgentWindow>>>);

impl DevAgentHook for Windows {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.0.lock().push(window);
    }
}

/// An application under test (ADR-0243): the `flui-testing` counterpart of
/// `flutter_test`'s `WidgetTester`.
///
/// ```
/// use flui_testing::{Tester, find};
/// use flui_widgets::{Center, Text};
///
/// let mut t = Tester::mount(Center::new().child(Text::new("Hello")));
/// t.expect(find::text("Hello")).to_exist();
/// ```
pub struct Tester {
    host: HeadlessHost,
    window: AgentWindow,
    lease: InputLease,
    /// Kept alive so the window keeps its agent: dropping it detaches.
    _agent: HeadlessDevAgent,
    /// The window this run mounted in, its storage the one the run keeps.
    config: TestWindow,
    /// The keys [`Tester::key_down`] holds, which a chord neither presses
    /// nor releases.
    held_keys: Vec<String>,
    /// This run of the application over its storage, whose held writes a
    /// kill discards.
    run: Option<RunId>,
    /// Mounts the same view again in a window: what [`Tester::restart`] runs.
    relaunch: Rc<dyn Fn(TestWindow) -> Tester>,
    /// Whether the application is on a trip to the background
    /// ([`Tester::background`]).
    in_background: bool,
}

impl std::fmt::Debug for Tester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tester")
            .field("window", &self.window.id())
            .finish_non_exhaustive()
    }
}

impl Tester {
    /// Mount `view` in an 800 by 600 window, as `run_app` mounts it, and run
    /// its first frame.
    ///
    /// # Panics
    ///
    /// When the first frame fails.
    #[track_caller]
    #[must_use]
    pub fn mount<V>(view: V) -> Self
    where
        V: View + Clone + 'static,
    {
        Self::mount_in(view, TestWindow::default())
    }

    /// Mount `view` in `window`, as `run_app` mounts it, and run its first
    /// frame.
    ///
    /// # Panics
    ///
    /// When the first frame fails.
    #[track_caller]
    #[must_use]
    pub fn mount_in<V>(view: V, window: TestWindow) -> Self
    where
        V: View + Clone + 'static,
    {
        let mut config = window;
        let mut headless = HeadlessWindow::new(config.width, config.height).with_text_input();
        if let Some(platform) = config.platform {
            headless = headless.with_platform(platform);
        }
        if config.in_browser {
            headless = headless.in_browser();
        }
        if let Some(handler) = config.close_request_handler.clone() {
            headless = headless.with_close_request_handler(handler);
        }
        let storage = match &config.storage {
            WindowStorage::Fresh => Some(MemoryStorage::new()),
            WindowStorage::Given(storage) => Some(storage.clone()),
            WindowStorage::Absent => None,
        };
        if let Some(storage) = &storage {
            // A restart launches over what this run leaves.
            config.storage = WindowStorage::Given(storage.clone());
        }
        let windows = Arc::new(Mutex::new(Vec::new()));
        let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
        let (run_storage, run) = storage.as_ref().map(MemoryStorage::for_run).unzip();
        let mut host = HeadlessHost::with_application(
            headless,
            run_storage.map(|storage| KitStorage {
                host: Arc::new(storage),
                provided: None,
            }),
            config.registrations.clone(),
        )
        .with_dev_agent(&mut agent);
        host.attach(&view)
            .expect("BUG: a fresh host has no root attached");
        let _ = host.pump(FRAME);
        let window = windows
            .lock()
            .pop()
            .expect("BUG: a host with an attached agent hands it its window");
        let lease = window
            .input_lease()
            .expect("BUG: a fresh window has an input lease to hand out");
        let mut tester = Self {
            host,
            window,
            lease,
            _agent: agent,
            config,
            held_keys: Vec::new(),
            run,
            relaunch: Rc::new(move |window| Self::mount_in(view.clone(), window)),
            in_background: false,
        };
        // The window collects semantics from the frame after its agent took
        // it; run that frame so the first locate reads a tree.
        tester.pump();
        tester
    }

    // --- Time -------------------------------------------------------------

    /// Run one frame, [`FRAME`] after the last.
    ///
    /// # Panics
    ///
    /// With the first failure the frame raised.
    #[track_caller]
    pub fn pump(&mut self) {
        let _ = self.host.pump(FRAME);
    }

    /// Advance the clock by `duration` in frames of [`FRAME`], the last one
    /// shorter when `duration` is not a whole number of frames.
    ///
    /// # Panics
    ///
    /// With the first failure a frame raised.
    #[track_caller]
    pub fn pump_for(&mut self, duration: Duration) {
        let mut left = duration;
        while !left.is_zero() {
            let step = left.min(FRAME);
            let _ = self.host.pump(step);
            left -= step;
        }
    }

    /// Run frames until the application asks for none and has nothing due,
    /// at most `max_frames`; how many it took.
    ///
    /// # Errors
    ///
    /// [`TestError::NotSettled`] when work remains, such as a repeating
    /// animation.
    pub fn try_pump_and_settle(&mut self, max_frames: usize) -> Result<usize, TestError> {
        self.host
            .pump_until_settled(FRAME, max_frames)
            .map_err(|_| TestError::NotSettled { frames: max_frames })
    }

    /// Run frames until the application settles, at most 300 of them (five
    /// seconds); how many it took.
    ///
    /// # Panics
    ///
    /// When it does not settle.
    #[track_caller]
    pub fn pump_and_settle(&mut self) -> usize {
        self.try_pump_and_settle(300)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The clock the application reads: frames, gesture deadlines and timers.
    #[must_use]
    pub fn clock(&self) -> &crate::HostClock {
        self.host.clock()
    }

    // --- Reading ----------------------------------------------------------

    /// Every element `locator` matches now, in tree order: the whole built
    /// tree, rows a list keeps past its viewport included, as `flutter_test`'s
    /// `find` sees them. Whether a match is shown is
    /// [`is_visible`](Self::is_visible), and an action on one that is not
    /// fails with `visible`.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window cannot answer.
    pub fn try_find_all(&mut self, locator: &Locator) -> Result<Vec<Match>, TestError> {
        self.run_due_now();
        let call = self
            .window
            .call::<SemanticsLocate>(LocateParams::new(locator.clone()))?;
        Ok(self.answered(call)?.matches)
    }

    /// Every element `locator` matches now, in tree order.
    ///
    /// # Panics
    ///
    /// When the window cannot answer.
    #[track_caller]
    #[must_use]
    pub fn find_all(&mut self, locator: &Locator) -> Vec<Match> {
        self.try_find_all(locator)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The one element `locator` names.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`] or [`TestError::Ambiguous`] unless exactly one
    /// element matches (or the locator picks one);
    /// [`TestError::UnpublishedIdentifier`] in place of `NotFound` when an
    /// identifier the locator names is on a node the read lifts into its
    /// parent.
    pub fn try_read(&mut self, locator: &Locator) -> Result<Node, TestError> {
        Ok(self.resolve(locator)?.node)
    }

    /// The one element `locator` names, as the semantics tree publishes it.
    ///
    /// # Panics
    ///
    /// Unless exactly one element matches.
    #[track_caller]
    #[must_use]
    pub fn read(&mut self, locator: &Locator) -> Node {
        self.try_read(locator)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Whether an element `locator` matches is visible: its bounds are not
    /// empty and lie partly inside the window and every clipping ancestor, the
    /// `visible` check an action makes. A row a list keeps past its viewport
    /// is matched and not visible. Whether a pointer there reaches it, past a
    /// barrier or a popup over it, is an action's `receives` check, not this.
    ///
    /// The locate and the reading of each match are separate reads, and a
    /// frame due between them may move or rebuild what the locate found: an
    /// answer counts only when every reading was made on the tree the locate
    /// read, and is taken again otherwise, a few times at most.
    ///
    /// # Errors
    ///
    /// When the window cannot answer, and [`TestError::KeptChanging`] when
    /// frames keep changing the tree between the locate and its readings.
    pub fn try_is_visible(&mut self, locator: &Locator) -> Result<bool, TestError> {
        'read: for _ in 0..VISIBLE_READS {
            for one in &self.try_find_all(locator)? {
                let facts = self.actionability(one)?;
                if facts.revision != one.revision {
                    continue 'read;
                }
                if facts.visible {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        Err(TestError::KeptChanging {
            reads: VISIBLE_READS,
        })
    }

    /// Whether an element `locator` matches is visible.
    ///
    /// # Panics
    ///
    /// As [`Self::try_is_visible`] fails.
    #[track_caller]
    #[must_use]
    pub fn is_visible(&mut self, locator: &Locator) -> bool {
        self.try_is_visible(locator)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The bounds of the one element `locator` names, in the window's logical
    /// pixels.
    ///
    /// # Errors
    ///
    /// As [`Self::try_read`], and [`TestError::NotActionable`] (`visible`) for
    /// an element with no bounds.
    pub fn try_rect(&mut self, locator: &Locator) -> Result<LogicalRect, TestError> {
        let found = self.resolve(locator)?;
        let facts = self.actionability(&found)?;
        facts.rect.ok_or_else(|| TestError::NotActionable {
            element: line_of(&found.node),
            check: ActionabilityCheck::Visible,
            covered_by: None,
        })
    }

    /// The bounds of the one element `locator` names, in the window's logical
    /// pixels: what `flutter_test`'s `getRect` answers.
    ///
    /// # Panics
    ///
    /// As [`Self::try_rect`] fails.
    #[track_caller]
    #[must_use]
    pub fn rect(&mut self, locator: &Locator) -> LogicalRect {
        self.try_rect(locator)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The window's semantics tree, as an agent reads it.
    ///
    /// # Panics
    ///
    /// When the window cannot answer.
    #[track_caller]
    #[must_use]
    pub fn semantics(&mut self) -> Tree {
        self.try_semantics()
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn try_semantics(&mut self) -> Result<Tree, TestError> {
        self.run_due_now();
        let call = self
            .window
            .call::<SemanticsRead>(ReadParams::new(self.window.id()))?;
        self.answered(call)
    }

    /// The window's widget (element) tree, as an agent inspects it.
    ///
    /// # Panics
    ///
    /// When the window cannot answer.
    #[track_caller]
    #[must_use]
    pub fn inspect_widgets(&mut self) -> InspectReading {
        self.run_due_now();
        let call = self
            .window
            .call::<InspectWidgets>(InspectParams::new(self.window.id()))
            .unwrap_or_else(|error| panic!("{}", TestError::from(error)));
        self.answered(call)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The window's render tree, as an agent inspects it.
    ///
    /// # Panics
    ///
    /// When the window cannot answer.
    #[track_caller]
    #[must_use]
    pub fn inspect_render(&mut self) -> InspectReading {
        self.run_due_now();
        let call = self
            .window
            .call::<InspectRender>(InspectParams::new(self.window.id()))
            .unwrap_or_else(|error| panic!("{}", TestError::from(error)));
        self.answered(call)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The draw commands the one element `locator` names painted in the
    /// window's last submitted scene, in paint order: the render object that
    /// forms the element and every render object below it, as `flutter_test`'s
    /// `paints` matcher reads one render object's painting. A painter that
    /// draws outside its own bounds is read whole, and a sibling's drawing is
    /// left out, even where it overlaps.
    ///
    /// Each command's `transform` places it in the window, its canvas
    /// transform composed with every offset and transform of the layers above
    /// it; a command under a follower its leader does not place is left out,
    /// as it is not painted. The frames due now run first, as for every read;
    /// a scene that painted nothing new keeps the last one.
    ///
    /// The agent's protocol carries no drawing, so the tester reads the scene
    /// the headless host received and selects its picture layers by the
    /// render objects the window's agent lists under the element. A subtree
    /// larger than one inspection lists is read in several: each node one
    /// cut is inspected a level deep, and each child it did not list is read
    /// from there, so the drawing is read whole. A frame that
    /// submits a scene between those reads starts the read again, and two
    /// such reads in a row that find the same render objects stand, as an
    /// animation submits a scene every frame.
    ///
    /// # Errors
    ///
    /// As [`Self::try_read`]; [`TestError::NoRenderObject`] when the element
    /// is an authored region no render object forms;
    /// [`TestError::TooManyChildren`] when one render object below it has
    /// more children than one inspection lists, rather than a partial
    /// drawing; [`TestError::KeptChanging`] when every read saw a new scene.
    pub fn try_draw_commands(
        &mut self,
        locator: &Locator,
    ) -> Result<Vec<flui_painting::DrawCommandBuf>, TestError> {
        // The subtree a read found when a frame ran during it: an animation
        // submits a scene on every frame without changing which render
        // objects the element has, so two reads that agree stand.
        let mut changed: Option<rustc_hash::FxHashSet<flui_foundation::RenderId>> = None;
        for _ in 0..VISIBLE_READS {
            self.run_due_now();
            let submits = self.host.sink().submits();
            let found = self.resolve(locator)?;
            // A render-formed node's id is its render object's packed id;
            // an authored region's carries a domain bit no render id has.
            let Some(render) = flui_foundation::RenderId::from_u64(found.element.element.get())
            else {
                return Err(TestError::NoRenderObject {
                    element: line_of(&found.node),
                });
            };
            let painters = self.render_subtree(render, &found.node);
            // A frame between the locate and the inspections may have
            // replaced what they read, its failure included: read again,
            // unless this read and the last agree.
            let painters = if self.host.sink().submits() == submits {
                painters?
            } else {
                match painters {
                    Ok(painters) if changed.as_ref() == Some(&painters) => painters,
                    other => {
                        changed = other.ok();
                        continue;
                    }
                }
            };
            return Ok(self.host.sink().layer_tree().map_or_else(Vec::new, |tree| {
                scene_commands(tree, |painter| painters.contains(&painter))
            }));
        }
        Err(TestError::KeptChanging {
            reads: VISIBLE_READS,
        })
    }

    /// `render` and every render object below it, read with as many
    /// inspections as the subtree takes.
    ///
    /// An inspection walks depth first on one node budget, so where it runs
    /// out every ancestor still open counts its unvisited children as
    /// omitted, and the protocol does not name them. Each node cut that way
    /// is inspected one level deep to name its children, and each child the
    /// first inspection did not list is the root of the next.
    fn render_subtree(
        &mut self,
        render: flui_foundation::RenderId,
        element: &Node,
    ) -> Result<rustc_hash::FxHashSet<flui_foundation::RenderId>, TestError> {
        let mut painters = rustc_hash::FxHashSet::default();
        let mut roots = vec![render];
        while let Some(root) = roots.pop() {
            let mut cut = Vec::new();
            let reading = self.inspect_render_from(root, None)?;
            let mut pending = vec![&reading];
            while let Some(node) = pending.pop() {
                pending.extend(&node.children);
                let id = render_of(node);
                painters.insert(id);
                if node.omitted_children > 0 {
                    cut.push(id);
                }
            }
            for parent in cut {
                let level = self.inspect_render_from(parent, Some(1))?;
                if level.omitted_children > 0 {
                    // Its children alone are more than one inspection lists.
                    return Err(TestError::TooManyChildren {
                        element: line_of(element),
                        children: level.omitted_children,
                    });
                }
                roots.extend(
                    level
                        .children
                        .iter()
                        .map(render_of)
                        .filter(|child| !painters.contains(child)),
                );
            }
        }
        Ok(painters)
    }

    /// One `inspect.render` reading from `root`, `max_depth` levels deep.
    fn inspect_render_from(
        &mut self,
        root: flui_foundation::RenderId,
        max_depth: Option<u32>,
    ) -> Result<flui_protocol::inspect::InspectNode, TestError> {
        let mut params = InspectParams::new(self.window.id());
        params.root = Some(flui_foundation::inspect::Link::Render(root).to_string());
        params.max_depth = max_depth;
        let call = self.window.call::<InspectRender>(params)?;
        let reading = self.answered(call)?;
        let [node] = <[flui_protocol::inspect::InspectNode; 1]>::try_from(reading.roots)
            .expect("BUG: an inspection from one root reads one root");
        Ok(node)
    }

    /// The draw commands the one element `locator` names painted, as
    /// [`Self::try_draw_commands`] reads them.
    ///
    /// # Panics
    ///
    /// As [`Self::try_draw_commands`] fails.
    #[track_caller]
    #[must_use]
    pub fn draw_commands(&mut self, locator: &Locator) -> Vec<flui_painting::DrawCommandBuf> {
        self.try_draw_commands(locator)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    // --- Acting -----------------------------------------------------------

    /// Tap the one element `locator` names at the centre of its visible part.
    ///
    /// # Errors
    ///
    /// As [`Self::try_read`]; [`TestError::NotActionable`] when the element is
    /// hidden, disabled or covered there.
    pub fn try_tap(&mut self, locator: &Locator) -> Result<(), TestError> {
        let at = self.action_point(locator)?;
        self.inject(vec![pointer(vec![
            move_to(at),
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ])])
    }

    /// Tap the one element `locator` names: a primary press and release at the
    /// centre of its visible part, through the window's own input path.
    ///
    /// # Panics
    ///
    /// As [`Self::try_tap`] fails.
    #[track_caller]
    pub fn tap(&mut self, locator: &Locator) {
        self.try_tap(locator)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Press the one element `locator` names for `duration`, then release.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    pub fn try_long_press(
        &mut self,
        locator: &Locator,
        duration: Duration,
    ) -> Result<(), TestError> {
        let at = self.action_point(locator)?;
        let pause = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        self.inject(vec![pointer(vec![
            move_to(at),
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Pause { duration_ms: pause },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ])])
    }

    /// Press the one element `locator` names for `duration` (a long press
    /// takes 500 ms), then release.
    ///
    /// # Panics
    ///
    /// As [`Self::try_tap`] fails.
    #[track_caller]
    pub fn long_press(&mut self, locator: &Locator, duration: Duration) {
        self.try_long_press(locator, duration)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Press on the one element `locator` names and move by `dx`, `dy`
    /// logical pixels over `duration`, then release.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    pub fn try_drag(
        &mut self,
        locator: &Locator,
        dx: f64,
        dy: f64,
        duration: Duration,
    ) -> Result<(), TestError> {
        let at = self.action_point(locator)?;
        self.try_drag_from(at, dx, dy, duration)
    }

    /// Press on the one element `locator` names and move by `dx`, `dy`
    /// logical pixels over `duration`, then release.
    ///
    /// # Panics
    ///
    /// As [`Self::try_tap`] fails.
    #[track_caller]
    pub fn drag(&mut self, locator: &Locator, dx: f64, dy: f64, duration: Duration) {
        self.try_drag(locator, dx, dy, duration)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Press at `start`, a point in the window's logical pixels, and move by
    /// `dx`, `dy` over `duration`, then release.
    ///
    /// This is for a gesture that starts at a place rather than on an
    /// element, such as an edge swipe. A drag that starts on an element is
    /// [`Self::drag`], which checks that the element can take it.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window refuses the input, as it refuses
    /// a non-finite point.
    pub fn try_drag_from(
        &mut self,
        start: LogicalPoint,
        dx: f64,
        dy: f64,
        duration: Duration,
    ) -> Result<(), TestError> {
        let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        self.inject(vec![pointer(vec![
            move_to(start),
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Move {
                to: LogicalPoint::new(start.x + dx, start.y + dy),
                duration_ms,
                pressure: None,
            },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ])])
    }

    /// Press at `start` and move by `dx`, `dy` over `duration`, then release.
    ///
    /// # Panics
    ///
    /// As [`Self::try_drag_from`] fails.
    #[track_caller]
    pub fn drag_from(&mut self, start: LogicalPoint, dx: f64, dy: f64, duration: Duration) {
        self.try_drag_from(start, dx, dy, duration)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Tap the one field `locator` names, then commit `text` into it as an
    /// input method commits text.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    pub fn try_enter_text(&mut self, locator: &Locator, text: &str) -> Result<(), TestError> {
        self.try_tap(locator)?;
        self.inject(vec![InputSource::Text {
            text: text.to_owned(),
        }])
    }

    /// Tap the one field `locator` names, then commit `text` into it.
    ///
    /// # Panics
    ///
    /// As [`Self::try_tap`] fails.
    #[track_caller]
    pub fn enter_text(&mut self, locator: &Locator, text: &str) {
        self.try_enter_text(locator, text)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Press `chords` in turn, as `element.press_keys` does, to the window's
    /// focused element.
    ///
    /// A chord is `+`-joined keys, each a W3C key name (`"Enter"`, `"Tab"`,
    /// `"ArrowDown"`, `"Control"`) or the text the key types (`"a"`, `"+"`):
    /// `"Control+a"`, `"Shift+Tab"`, `"Shift++"`. A chord presses its keys left
    /// to right and releases them right to left. A key held with
    /// [`Self::key_down`] stays held, so it modifies every chord pressed
    /// meanwhile; a chord that names it neither presses nor releases it.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when a key is neither a key name nor the text of
    /// one, as in `"Control+"`; nothing is pressed then.
    pub fn try_press_keys(&mut self, chords: &[&str]) -> Result<(), TestError> {
        let mut actions = Vec::new();
        for chord in chords {
            let mut keys = chord_keys(chord);
            keys.retain(|key| !self.held_keys.iter().any(|held| held == key));
            actions.extend(keys.iter().map(|key| KeyAction::Down {
                key: (*key).to_owned(),
            }));
            actions.extend(keys.iter().rev().map(|key| KeyAction::Up {
                key: (*key).to_owned(),
            }));
        }
        self.inject(vec![keyboard(actions)])
    }

    /// Press `chords` in turn to the window's focused element: `"Enter"`,
    /// `"Control+a"`, `"Shift+Tab"`.
    ///
    /// # Panics
    ///
    /// As [`Self::try_press_keys`] fails.
    #[track_caller]
    pub fn press_keys(&mut self, chords: &[&str]) {
        self.try_press_keys(chords)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Press `key` and hold it until [`Self::key_up`]: a modifier held across
    /// several chords, or a key held through its auto-repeats. Pressing a key
    /// already held repeats it, as a platform repeats a held key and as
    /// WebDriver dispatches a second `keyDown`.
    ///
    /// # Errors
    ///
    /// As [`Self::try_press_keys`].
    pub fn try_key_down(&mut self, key: &str) -> Result<(), TestError> {
        self.inject(vec![keyboard(vec![KeyAction::Down {
            key: key.to_owned(),
        }])])?;
        if !self.held_keys.iter().any(|held| held == key) {
            self.held_keys.push(key.to_owned());
        }
        Ok(())
    }

    /// Press `key` and hold it until [`Self::key_up`]; pressing it again
    /// while held is an auto-repeat.
    ///
    /// # Panics
    ///
    /// As [`Self::try_key_down`] fails.
    #[track_caller]
    pub fn key_down(&mut self, key: &str) {
        self.try_key_down(key)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Release `key`, held since [`Self::key_down`]. Releasing a key that is
    /// not held sends nothing.
    ///
    /// # Errors
    ///
    /// As [`Self::try_press_keys`].
    pub fn try_key_up(&mut self, key: &str) -> Result<(), TestError> {
        self.inject(vec![keyboard(vec![KeyAction::Up {
            key: key.to_owned(),
        }])])?;
        self.held_keys.retain(|held| held != key);
        Ok(())
    }

    /// Release `key`, held since [`Self::key_down`].
    ///
    /// # Panics
    ///
    /// As [`Self::try_key_up`] fails.
    #[track_caller]
    pub fn key_up(&mut self, key: &str) {
        self.try_key_up(key)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Show `text` as the focused field's input-method composition, with the
    /// caret `caret` characters into it, or with no caret when `caret` is
    /// `None`, as an input method that converts shows none. The composition
    /// replaces the one shown before, or the selection when there is none.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when `caret` is past the end of `text`.
    pub fn try_compose(&mut self, text: &str, caret: Option<u32>) -> Result<(), TestError> {
        self.inject_ime(ImeAction::SetComposition {
            text: text.to_owned(),
            cursor: caret,
            caret_hidden: caret.is_none(),
        })
    }

    /// Show `text` as the focused field's composition, the caret `caret`
    /// characters into it, or hidden when `None`.
    ///
    /// # Panics
    ///
    /// As [`Self::try_compose`] fails.
    #[track_caller]
    pub fn compose(&mut self, text: &str, caret: Option<u32>) {
        self.try_compose(text, caret)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// End the focused field's composition by committing `text` in its
    /// place, or insert `text` at the selection when nothing is composed.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window refuses the input.
    pub fn try_commit_composition(&mut self, text: &str) -> Result<(), TestError> {
        self.inject_ime(ImeAction::Commit {
            text: text.to_owned(),
        })
    }

    /// End the focused field's composition by committing `text`.
    ///
    /// # Panics
    ///
    /// As [`Self::try_commit_composition`] fails.
    #[track_caller]
    pub fn commit_composition(&mut self, text: &str) {
        self.try_commit_composition(text)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// End the focused field's composition without committing: its text goes
    /// and the caret returns to where it began.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window refuses the input.
    pub fn try_cancel_composition(&mut self) -> Result<(), TestError> {
        self.inject_ime(ImeAction::Cancel)
    }

    /// End the focused field's composition without committing.
    ///
    /// # Panics
    ///
    /// As [`Self::try_cancel_composition`] fails.
    #[track_caller]
    pub fn cancel_composition(&mut self) {
        self.try_cancel_composition()
            .unwrap_or_else(|error| panic!("{error}"));
    }

    // --- The application's window and storage -----------------------------

    /// The storage the application keeps its documents in, or `None` for a
    /// window [`without_storage`](TestWindow::without_storage). A
    /// [`restart`](Self::restart) launches over it.
    #[must_use]
    pub fn storage(&self) -> Option<&MemoryStorage> {
        match &self.config.storage {
            WindowStorage::Given(storage) => Some(storage),
            WindowStorage::Fresh | WindowStorage::Absent => None,
        }
    }

    /// End this run by force and launch the same view in the same window over
    /// the same storage, as when the application is made to quit and is
    /// opened again. Nothing is asked, unlike a quit the user asks for
    /// ([`request_quit`](Self::request_quit)): the window commits its
    /// composition and closes past every hold, what the
    /// documents published is written (a write the storage holds is not
    /// waited for: it lands when the hold ends, ahead of the next run's,
    /// which read the storage without it and so conflicts with it, as on a
    /// disk that had not finished the quit write; what a
    /// document still owes then is lost with the run), and the old
    /// tree, its states and its runtime are gone before the new one mounts.
    ///
    /// # Panics
    ///
    /// When code the quit runs panics (its first panic resumes once the write
    /// is done), or when the new run's first frame fails.
    #[must_use]
    pub fn restart(mut self) -> Self {
        self.host.quit_now();
        let relaunch = Rc::clone(&self.relaunch);
        let config = self.config.clone();
        drop(self);
        relaunch(config)
    }

    /// The process is killed and the application opened again over the same
    /// storage: no composition is committed, no close is asked and nothing
    /// more is written, so a write the run accepted and had not carried out
    /// is lost, a write the storage holds unstarted or uncommitted included,
    /// and the storage refuses whatever the run still publishes, from its
    /// teardown or a storage handle of the run that outlives it, with
    /// [`StorageError::Cancelled`](flui_platform::StorageError::Cancelled);
    /// another run's write over the same storage is kept, and so is the
    /// test's own. The old run is dropped as a host drops its UI runtime: its
    /// states are disposed and its observers see it detach, which a real kill
    /// never runs. The old tree is gone before the new one mounts.
    /// [`Self::restart`] is the quit that writes; dropping a `Tester` ends the
    /// run without either, its held writes left to the storage.
    ///
    /// # Panics
    ///
    /// When code the old run's teardown runs panics, or when the new run's
    /// first frame fails. The run's writes are discarded before the teardown,
    /// so a teardown panic loses them all the same.
    #[must_use]
    pub fn relaunch_after_kill(self) -> Self {
        let relaunch = Rc::clone(&self.relaunch);
        let config = self.config.clone();
        // The run dies before its teardown starts, so nothing the teardown
        // writes or releases reaches the storage, a panic in it included.
        if let (Some(storage), Some(run)) = (self.storage(), self.run) {
            storage.kill_run(run);
        }
        drop(self);
        relaunch(config)
    }

    /// The user closes the window. The window's close guard is asked first:
    /// a close it refuses is recorded, nothing is torn down, and the frame
    /// after its last hold is released carries the close out. `true` when the
    /// window closed now.
    pub fn request_close(&mut self) -> bool {
        let window = self.host.primary_window();
        let closed = self.host.request_close(window, CloseReason::User);
        self.run_due_now();
        closed
    }

    /// Whether the window has closed: at once, once what held its close was
    /// released or decided, or because the session ended. A close refused
    /// and still recorded is not one.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.host.is_closed(self.host.primary_window())
    }

    /// The user quits the application, as `AppHandle::request_quit` and
    /// AppKit's Dock Quit ask it (ADR-0350 §1): the frame after asks the
    /// window, as for the program's close; a refusal its guard records
    /// defers the quit until the hold is released, a refusal without one
    /// cancels it and leaves the window as it was, and a quit nothing holds
    /// closes the window, writes what the documents published and ends the
    /// run ([`is_closed`](Self::is_closed)).
    pub fn request_quit(&mut self) {
        self.host.request_quit();
        self.run_due_now();
    }

    /// The operating system ends the user's session, as the platform the
    /// window names does it ([`HeadlessHost::end_session`]): on a desktop the
    /// session-ended notice reaches the application, the window commits its
    /// composition and closes past every hold, as a session that is ending no
    /// longer asks, and what the documents published is written, as in
    /// [`Self::restart`] (a write the storage holds is not waited for, and
    /// what any document still owes then is lost); on a phone the notice and
    /// the write leave the window open. Returns the application's answer:
    /// [`Block`](flui_platform::SessionEndAnswer::Block) on a phone while
    /// writes are in flight.
    ///
    /// In a window that plays a browser's page
    /// ([`TestWindow::in_browser`]), on any build target, the session's end
    /// is its `pagehide`, as a runner built for the web answers it: the
    /// notice alone reaches the application, and the window stays open with
    /// nothing written and nothing closed.
    pub fn end_session(&mut self) -> flui_platform::SessionEndAnswer {
        self.host
            .end_session(flui_platform::SessionEndPhase::Ending)
    }

    /// The window becomes hidden (`false`) or shown again (`true`), as when
    /// the user minimizes it or another window covers it on a desktop; then
    /// the frame that reports it runs. Its focus and whether it may run stay
    /// as they were: a phone's trip to the background is
    /// [`background`](Self::background).
    pub fn set_window_visible(&mut self, visible: bool) {
        self.host.set_window_visible(visible);
        self.run_due_now();
    }

    /// The application leaves the screen on a phone, in the order Android
    /// and UIKit report it: the window loses the focus, its execution is
    /// suspended, it is hidden, and then the session's end is announced once
    /// for the trip: the session-ended notice arrives and what the documents
    /// published is written (ADR-0350 §2). The window stays open;
    /// [`foreground`](Self::foreground) brings it back. Then the frame that
    /// reports it runs. Returns the application's answer to the session's
    /// end: [`Block`](flui_platform::SessionEndAnswer::Block) while writes are
    /// in flight, released once they end
    /// ([`HeadlessHost::session_end_releases`]).
    ///
    /// # Errors
    ///
    /// [`TestError::NotAPhone`] for a window that names no phone (only
    /// Android and iOS leave the screen this way), and
    /// [`TestError::InBackground`] while the application is already in the
    /// background: a phone reports no second trip before a return.
    ///
    /// # Panics
    ///
    /// With the first panic of a notice subscriber or the write.
    pub fn try_background(&mut self) -> Result<flui_platform::SessionEndAnswer, TestError> {
        self.phone()?;
        if self.in_background {
            return Err(TestError::InBackground);
        }
        self.in_background = true;
        let window = self.host.primary_window();
        self.host.set_window_focused(false);
        self.host
            .set_window_execution(window, flui_platform::WindowExecutionState::Suspended);
        self.host.set_window_visible(false);
        let answer = self
            .host
            .end_session(flui_platform::SessionEndPhase::Ending);
        self.run_due_now();
        Ok(answer)
    }

    /// [`try_background`](Self::try_background), panicking at its caller
    /// with its refusal.
    ///
    /// # Panics
    ///
    /// When `try_background` refuses, or as it panics.
    #[track_caller]
    pub fn background(&mut self) -> flui_platform::SessionEndAnswer {
        self.try_background()
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The application comes back to the screen on a phone, in the order
    /// Android and UIKit report it: the window is shown, it may run again,
    /// and it takes the focus. Then the frame that reports it runs.
    ///
    /// # Errors
    ///
    /// [`TestError::NotAPhone`] for a window that names no phone, and
    /// [`TestError::InForeground`] when the application is not in the
    /// background.
    pub fn try_foreground(&mut self) -> Result<(), TestError> {
        self.phone()?;
        if !self.in_background {
            return Err(TestError::InForeground);
        }
        self.in_background = false;
        let window = self.host.primary_window();
        self.host.set_window_visible(true);
        self.host
            .set_window_execution(window, flui_platform::WindowExecutionState::Running);
        self.host.set_window_focused(true);
        self.run_due_now();
        Ok(())
    }

    /// [`try_foreground`](Self::try_foreground), panicking at its caller
    /// with its refusal.
    ///
    /// # Panics
    ///
    /// When `try_foreground` refuses.
    #[track_caller]
    pub fn foreground(&mut self) {
        self.try_foreground()
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// `Ok` when the window names a phone.
    fn phone(&self) -> Result<(), TestError> {
        match self.config.platform {
            Some(flui_platform::TargetPlatform::Android | flui_platform::TargetPlatform::iOS) => {
                Ok(())
            }
            platform => Err(TestError::NotAPhone {
                platform: platform.unwrap_or(flui_platform::TargetPlatform::Unknown),
            }),
        }
    }

    /// The window loses (`false`) or regains (`true`) the operating system's
    /// focus, as when the user switches to another application and back;
    /// then the frame that reports it runs.
    pub fn set_window_focused(&mut self, focused: bool) {
        self.host.set_window_focused(focused);
        self.run_due_now();
    }

    // --- Expecting --------------------------------------------------------

    /// What a test expects of the elements `locator` matches. Each assertion
    /// waits while the application works ([module docs](self#time)).
    pub fn expect(&mut self, locator: Locator) -> Expectation<'_> {
        Expectation {
            tester: self,
            locator,
            timeout: EXPECT_TIMEOUT,
        }
    }

    // --- Internals --------------------------------------------------------

    /// Run the frames due now, without moving the clock, so the read that
    /// follows sees what they commit: a read is answered at the drain that
    /// opens a frame, before that frame runs ready work. Bounded, because a
    /// running animation keeps a frame due.
    fn run_due_now(&mut self) {
        for _ in 0..DUE_NOW_FRAMES {
            if self.host.time_to_next_frame() != Some(Duration::ZERO) {
                return;
            }
            let _ = self.host.pump(Duration::ZERO);
        }
    }

    /// Pump until `answer` is answered: the drain at the next frame boundary,
    /// or a frame per input tick.
    fn answered<T>(&mut self, answer: AgentAnswer<T>) -> Result<T, TestError> {
        self.answered_within(answer, ANSWER_FRAMES)
    }

    /// Pump until `answer` is answered, at most `frames` frames after the
    /// first drain.
    fn answered_within<T>(
        &mut self,
        mut answer: AgentAnswer<T>,
        frames: u32,
    ) -> Result<T, TestError> {
        let _ = self.host.pump(Duration::ZERO);
        for _ in 0..frames {
            if let Some(answer) = answer.try_take() {
                return answer.map_err(TestError::from);
            }
            let _ = self.host.pump(FRAME);
        }
        match answer.try_take() {
            Some(answer) => answer.map_err(TestError::from),
            None => Err(TestError::Unanswered { frames }),
        }
    }

    /// The one match of `locator`, or why there is not one.
    fn resolve(&mut self, locator: &Locator) -> Result<Match, TestError> {
        let mut matches = self.try_find_all(locator)?;
        match matches.len() {
            0 => Err(self.not_found(locator)),
            1 => Ok(matches.remove(0)),
            count => Err(TestError::Ambiguous {
                locator: Box::new(locator.clone()),
                count,
                candidates: matches
                    .iter()
                    .take(SHOWN_LINES)
                    .map(|found| line_of(&found.node))
                    .collect::<Vec<String>>()
                    .join("\n"),
            }),
        }
    }

    /// Why nothing matches `locator`: an identifier it or a scope of it names
    /// is on a node the window reports as lifted, or else nothing carries it.
    fn not_found(&mut self, locator: &Locator) -> TestError {
        self.unpublished_identifier(locator)
            .unwrap_or_else(|| TestError::NotFound {
                locator: Box::new(locator.clone()),
                shown: self.shown(),
            })
    }

    /// [`TestError::UnpublishedIdentifier`] when `locator`, which matched
    /// nothing, names an identifier the window reports on a node it lifts
    /// away: its own identifier, or a scope's (`within`, `has`) that no
    /// published node carries. A scope a published node satisfies leaves the
    /// miss to the rest of the locator (ADR-0240 §4 lets one identifier stand
    /// in several places); the target's own identifier is blamed even then,
    /// since the lifted instance may be the one its scope holds. It only
    /// explains a failure, so a misplaced guess costs a hint, never a verdict.
    fn unpublished_identifier(&mut self, locator: &Locator) -> Option<TestError> {
        let mut scopes: Vec<&str> = Vec::new();
        let mut pending: Vec<&Locator> = Vec::new();
        pending.extend(locator.within.as_deref());
        pending.extend(locator.has.as_deref());
        while let Some(scope) = pending.pop() {
            scopes.extend(scope.identifier.as_deref());
            pending.extend(scope.within.as_deref());
            pending.extend(scope.has.as_deref());
        }
        if locator.identifier.is_none() && scopes.is_empty() {
            return None;
        }
        let tree = self.try_semantics().ok()?;
        scopes.retain(|identifier| !carries_identifier(&tree.roots, identifier));
        let mut wanted = scopes;
        wanted.extend(locator.identifier.as_deref());
        // A node lifted inside a control is preferred, since that fix
        // differs.
        let mut lifted = tree
            .diagnostics
            .iter()
            .filter_map(|diagnostic| match diagnostic {
                TreeDiagnostic::UnpublishedIdentifier { identifier, parent }
                    if wanted.contains(&identifier.as_str()) =>
                {
                    Some((identifier, *parent))
                }
                _ => None,
            })
            .peekable();
        let first = lifted.peek().map(|(identifier, _)| (*identifier).clone())?;
        let (identifier, control) = lifted
            .find_map(|(identifier, parent)| {
                let control =
                    find_node(&tree.roots, parent).filter(|node| names_from_contents(node.role))?;
                Some((identifier.clone(), Some(line_of(control))))
            })
            .unwrap_or((first, None));
        Some(TestError::UnpublishedIdentifier {
            locator: Box::new(locator.clone()),
            identifier,
            control,
            shown: self.shown(),
        })
    }

    fn actionability(&mut self, found: &Match) -> Result<Actionability, TestError> {
        self.run_due_now();
        let call = self
            .window
            .call::<ElementActionability>(ActionabilityParams::new(found.element))?;
        self.answered(call)
    }

    /// Where an action on the one element `locator` names lands, once every
    /// actionability check holds.
    fn action_point(&mut self, locator: &Locator) -> Result<LogicalPoint, TestError> {
        let found = self.resolve(locator)?;
        let facts = self.actionability(&found)?;
        let failed = [
            (facts.attached, ActionabilityCheck::Attached),
            (facts.visible, ActionabilityCheck::Visible),
            (facts.enabled, ActionabilityCheck::Enabled),
            (facts.receives, ActionabilityCheck::Receives),
        ]
        .into_iter()
        .find_map(|(holds, check)| (!holds).then_some(check));
        match (failed, facts.point) {
            (None, Some(point)) => Ok(point),
            (failed, _) => Err(TestError::NotActionable {
                element: line_of(&found.node),
                check: failed.unwrap_or(ActionabilityCheck::Visible),
                covered_by: facts.covered_by,
            }),
        }
    }

    /// Deliver one input-method step to the focused field.
    fn inject_ime(&mut self, step: ImeAction) -> Result<(), TestError> {
        self.inject(vec![InputSource::Ime {
            actions: vec![step],
        }])
    }

    /// Deliver `sources` through the window's input lease and run the frame
    /// after the last tick.
    fn inject(&mut self, sources: Vec<InputSource>) -> Result<(), TestError> {
        // The agent plays a tick per frame, and a pause or a timed move for
        // its whole duration, before it answers, so the budget is the ticks
        // and that span in frames on top of the answer's own.
        let sequence = ActionSequence::new(sources);
        let frame_ms = FRAME.as_millis();
        let span_frames =
            u32::try_from(sequence.span().as_millis().div_ceil(frame_ms)).unwrap_or(u32::MAX);
        let ticks = u32::try_from(sequence.ticks()).unwrap_or(u32::MAX);
        let call = self
            .lease
            .call::<InputInject>(InputParams::new(self.lease.window(), sequence))?;
        let budget = ANSWER_FRAMES
            .saturating_add(ticks)
            .saturating_add(span_frames);
        let outcome = self.answered_within(call, budget);
        if matches!(outcome, Err(TestError::Unanswered { .. })) {
            // The keys and contacts the sequence holds are let go once it ends:
            // the release is queued behind it. A sequence still unfinished
            // after a second budget keeps them held, and the test has
            // already failed with this error.
            if let Ok(release) = self
                .lease
                .call::<InputRelease>(WindowParams::new(self.lease.window()))
            {
                let _released = self.answered_within(release, budget);
            }
            self.held_keys.clear();
        }
        outcome.map(|_outcome| ())
    }

    /// The window's outline, cut to its first lines, for a failure.
    fn shown(&mut self) -> String {
        match self.try_semantics() {
            Ok(tree) => {
                let full = outline(&tree.roots);
                let mut lines: Vec<&str> = full.lines().take(SHOWN_LINES).collect();
                if full.lines().count() > SHOWN_LINES {
                    lines.push("…");
                }
                if lines.is_empty() {
                    "(an empty semantics tree)".to_owned()
                } else {
                    lines.join("\n")
                }
            }
            Err(error) => format!("(the semantics tree could not be read: {error})"),
        }
    }
}

/// What a test expects of the elements a locator matches, from
/// [`Tester::expect`].
#[must_use = "an expectation asserts nothing until one of its methods runs"]
pub struct Expectation<'t> {
    tester: &'t mut Tester,
    locator: Locator,
    timeout: Duration,
}

impl std::fmt::Debug for Expectation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expectation")
            .field("locator", &self.locator)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Expectation<'_> {
    /// Wait at most `timeout` while the application works, instead of
    /// [`EXPECT_TIMEOUT`].
    pub fn within(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Exactly one element matches: `findsOneWidget`.
    #[track_caller]
    pub fn to_exist(self) {
        self.to_have_count(1);
    }

    /// No element matches: `findsNothing`. A node whose identifier the read
    /// lifts away matches nothing, so this passes for it: give such a node a
    /// role to find it (ADR-0240 §3).
    #[track_caller]
    pub fn to_be_gone(self) {
        self.to_have_count(0);
    }

    /// `count` elements match: `findsNWidgets`.
    #[track_caller]
    pub fn to_have_count(self, count: usize) {
        self.check(&format!("{count} matches"), |_, matches| {
            (matches.len() == count)
                .then_some(())
                .ok_or_else(|| format!("{}", matches.len()))
        });
    }

    /// The one element that matches has the accessible name `name`.
    #[track_caller]
    pub fn to_have_name(self, name: &str) {
        self.one(&format!("the name {name:?}"), |node| {
            node.name.as_deref() == Some(name)
        });
    }

    /// The one element that matches has the value `value`.
    #[track_caller]
    pub fn to_have_value(self, value: &str) {
        self.one(&format!("the value {value:?}"), |node| {
            node.value.as_deref() == Some(value)
        });
    }

    /// The one element that matches is checked.
    #[track_caller]
    pub fn to_be_checked(self) {
        self.one("checked", |node| node.checked == Some(Checked::True));
    }

    /// The one element that matches is not disabled, itself or through an
    /// ancestor.
    #[track_caller]
    pub fn to_be_enabled(self) {
        self.enabled_is(true);
    }

    /// The one element that matches is disabled, itself or through an
    /// ancestor.
    #[track_caller]
    pub fn to_be_disabled(self) {
        self.enabled_is(false);
    }

    /// Exactly one element matches and its effective enabled state, read
    /// with `element.actionability` as an action reads it, is `enabled`.
    #[track_caller]
    fn enabled_is(self, enabled: bool) {
        let expected = if enabled { "enabled" } else { "disabled" };
        self.check(expected, |tester, matches| match matches {
            [only] => match tester.actionability(only) {
                Ok(facts) if facts.enabled == enabled => Ok(()),
                Ok(_) => Err(line_of(&only.node)),
                Err(error) => Err(error.to_string()),
            },
            many => Err(format!("{} matches", many.len())),
        });
    }

    /// The one element that matches holds keyboard focus.
    #[track_caller]
    pub fn to_be_focused(self) {
        self.one("focused", |node| node.focused);
    }

    /// Exactly one element matches and `holds` is true of it.
    #[track_caller]
    fn one(self, expected: &str, holds: impl Fn(&Node) -> bool) {
        self.check(expected, |_, matches| match matches {
            [only] if holds(&only.node) => Ok(()),
            [only] => Err(line_of(&only.node)),
            many => Err(format!("{} matches", many.len())),
        });
    }

    /// Wait while the application works until `holds` answers `Ok` for the
    /// matches; `Err` carries what was found instead.
    #[track_caller]
    fn check(self, expected: &str, holds: impl Fn(&mut Tester, &[Match]) -> Result<(), String>) {
        let Self {
            tester,
            locator,
            timeout,
        } = self;
        if let Err(error) = wait(tester, &locator, timeout, expected, holds) {
            panic!("{error}");
        }
    }
}

fn wait(
    tester: &mut Tester,
    locator: &Locator,
    timeout: Duration,
    expected: &str,
    holds: impl Fn(&mut Tester, &[Match]) -> Result<(), String>,
) -> Result<(), TestError> {
    let mut waited = Duration::ZERO;
    loop {
        // Each read first runs the work due now, so both verdicts below are
        // made on the tree that work committed.
        let matches = tester.try_find_all(locator)?;
        let found = match holds(tester, &matches) {
            Ok(()) => return Ok(()),
            Err(found) => found,
        };
        // Only a frame due now keeps the expectation pumping: a timer due
        // later is the test's to reach with `pump_for`.
        let next = tester.host.time_to_next_frame();
        let gives_up = next != Some(Duration::ZERO) || waited >= timeout;
        // An expectation that gives up matching nothing, on an identifier
        // nothing selects, says why, as a lookup does. The reason only
        // explains a failure: an expectation that holds by matching nothing
        // (`to_be_gone`) passes, whatever lifted node carries the identifier.
        if gives_up
            && matches.is_empty()
            && let Some(error) = tester.unpublished_identifier(locator)
        {
            return Err(error);
        }
        if next != Some(Duration::ZERO) {
            return Err(TestError::Idle {
                expected: expected.to_owned(),
                locator: Box::new(locator.clone()),
                found,
                next_deadline: next,
                shown: tester.shown(),
            });
        }
        if waited >= timeout {
            return Err(TestError::Timeout {
                expected: expected.to_owned(),
                locator: Box::new(locator.clone()),
                found,
                timeout,
                shown: tester.shown(),
            });
        }
        tester.pump();
        waited += FRAME;
    }
}

/// What [`TestError::UnpublishedIdentifier`] tells the author to do: give
/// the identifier to the control the node is lifted into, since a group
/// there would cut the name the control's label gives it, or else give the
/// node a role.
fn unpublished_identifier_fix(control: Option<&str>) -> String {
    match control {
        Some(control) => format!(
            "Give the identifier to the control it is inside, `{control}`: a group there can \
             cut the name its label gives it."
        ),
        None => "Give that node a role other than generic or presentation, as \
                 `Semantics::new().container(true).role(SemanticsRole::Group).identifier(..)`."
            .to_owned(),
    }
}

/// Whether AccessKit names a node of `role` from the labels beneath it, so a
/// group between them would cut its name.
fn names_from_contents(role: Role) -> bool {
    matches!(
        role,
        Role::Button | Role::CheckBox | Role::RadioButton | Role::Link | Role::MenuItem
    )
}

/// Whether a node under `roots` is published with `identifier`.
fn carries_identifier(roots: &[Node], identifier: &str) -> bool {
    let mut pending: Vec<&Node> = roots.iter().collect();
    while let Some(node) = pending.pop() {
        if node.identifier.as_deref() == Some(identifier) {
            return true;
        }
        pending.extend(&node.children);
    }
    false
}

/// The node `id` names among `roots` and their descendants.
fn find_node(roots: &[Node], id: ElementId) -> Option<&Node> {
    let mut pending: Vec<&Node> = roots.iter().collect();
    while let Some(node) = pending.pop() {
        if node.id == id {
            return Some(node);
        }
        pending.extend(&node.children);
    }
    None
}

/// `node` as one outline line.
/// The render object an `inspect.render` node names.
fn render_of(node: &flui_protocol::inspect::InspectNode) -> flui_foundation::RenderId {
    match node.id.parse::<flui_foundation::inspect::Link>() {
        Ok(flui_foundation::inspect::Link::Render(id)) => id,
        _ => panic!(
            "BUG: inspect.render names its nodes by render handle, not {:?}",
            node.id
        ),
    }
}

fn line_of(node: &Node) -> String {
    outline(std::slice::from_ref(node))
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn pointer(actions: Vec<PointerAction>) -> InputSource {
    InputSource::Pointer {
        id: "tester".to_owned(),
        // A touch contact, as `flutter_test` taps and drags: a scroll view's
        // default behaviour drags by touch, not by mouse.
        device: PointerDevice::Touch,
        actions,
    }
}

fn move_to(at: LogicalPoint) -> PointerAction {
    PointerAction::Move {
        to: at,
        duration_ms: 0,
        pressure: None,
    }
}

fn keyboard(actions: Vec<KeyAction>) -> InputSource {
    InputSource::Keys {
        id: "tester".to_owned(),
        actions,
    }
}

/// The keys of `chord`, `+`-joined; a `+` key is the last one, as in
/// `"Shift++"`, or the whole chord.
fn chord_keys(chord: &str) -> Vec<&str> {
    if chord == "+" {
        return vec!["+"];
    }
    match chord.strip_suffix("++") {
        Some(held) if !held.is_empty() => held.split('+').chain(["+"]).collect(),
        _ => chord.split('+').collect(),
    }
}

/// The draw commands of `tree`'s picture layers whose painter `selects`, in
/// paint order, each placed in the tree's root coordinates; what
/// [`Tester::draw_commands`] reads.
///
/// Pre-order is paint order: a parent keeps its children in the order they
/// paint, a repaint boundary's content under its slot. An explicit stack,
/// children pushed last first, so a deep chain of effects cannot overflow
/// the test thread's stack. A follower its leader does not place is skipped
/// with its subtree, as the engine skips it.
fn scene_commands(
    tree: &flui_rendering::layer::LayerTree,
    selects: impl Fn(flui_foundation::RenderId) -> bool,
) -> Vec<flui_painting::DrawCommandBuf> {
    use flui_foundation::geometry::Matrix4;
    use flui_rendering::layer::{Layer, resolve_follower_transform};

    let mut commands = Vec::new();
    let mut pending = vec![(tree.root(), Matrix4::IDENTITY)];
    while let Some((id, above)) = pending.pop() {
        let Some(node) = tree.get(id) else { continue };
        let local = match node.layer() {
            Layer::Picture(picture) => {
                // A picture is one render object's run (ADR-0304 §2); one no
                // render object painted is no element's drawing.
                if !node.render_id().is_some_and(&selects) {
                    continue;
                }
                commands.extend(picture.picture().iter().map(|command| {
                    let mut command = command.to_buf();
                    command.transform = above * command.transform;
                    command
                }));
                Matrix4::IDENTITY
            }
            Layer::Transform(layer) => *layer.transform(),
            Layer::Follower(_) => match resolve_follower_transform(tree, id) {
                Some(transform) => transform,
                None => continue,
            },
            layer => {
                let offset = layer.local_translation();
                Matrix4::translation(offset.dx, offset.dy, 0.0)
            }
        };
        let placed = above * local;
        pending.extend(node.children().iter().rev().map(|&child| (child, placed)));
    }
    commands
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
    use flui_painting::{Canvas, DrawOp, paint::Paint, styling::Color};
    use flui_rendering::layer::{
        FollowerLayer, Layer, LayerLink, LayerNode, LayerTree, LeaderLayer, OffsetLayer,
        PictureLayer,
    };

    /// A one-pixel swatch of `shade`, painted by a render object of that slot.
    fn swatch(shade: u8) -> LayerNode {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_origin_size(Point::ZERO, Size::new(1.0, 1.0)),
            &Paint::fill(Color::rgb(shade, 0, 0)),
        );
        LayerNode::new(Layer::Picture(PictureLayer::new(canvas.finish()))).with_render(
            flui_foundation::RenderId::new_gen(
                u32::from(shade),
                flui_foundation::generation::Generation::FIRST,
            ),
        )
    }

    fn placed(tree: &LayerTree) -> Vec<(u8, Matrix4)> {
        super::scene_commands(tree, |_| true)
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Rect { paint, .. } => {
                    let shade = (paint.color.to_rgba_f32_array()[0] * 255.0).round();
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a channel the test wrote as a u8"
                    )]
                    let shade = shade as u8;
                    Some((shade, command.transform))
                }
                _ => None,
            })
            .collect()
    }

    /// A follower is placed by its leader, and one with no leader is left
    /// out with its subtree while the drawing after it is still read.
    ///
    /// Fails if the walk places a follower by its own translation (zero),
    /// takes its resolved transform as the window's or applies the
    /// follower's ancestors twice, reads an unplaced follower's subtree, or
    /// stops at it.
    #[test]
    fn followers_are_placed_by_their_leader_or_left_out() {
        let led = LayerLink::new();
        let unled = LayerLink::new();
        let mut tree = LayerTree::new(Layer::Offset(OffsetLayer::zero()));
        let root = tree.root();
        tree.push_child(
            root,
            Layer::Leader(LeaderLayer::with_offset(
                led,
                Size::new(5.0, 5.0),
                Offset::new(30.0, 40.0),
            )),
        );
        // Under a slot of its own, as an overlay boundary is: the resolved
        // transform is relative to that slot, which the walk applies above it.
        let slot = tree.push_child(
            root,
            Layer::Offset(OffsetLayer::new(Offset::new(100.0, 0.0))),
        );
        let follower = tree.push_child(slot, Layer::Follower(FollowerLayer::new(led)));
        tree.push_child(follower, swatch(1));
        let hidden = tree.push_child(
            root,
            Layer::Follower(FollowerLayer::new(unled).with_show_when_unlinked(false)),
        );
        tree.push_child(hidden, swatch(2));
        tree.push_child(root, swatch(3));

        assert_eq!(
            placed(&tree),
            [
                (1, Matrix4::translation(30.0, 40.0, 0.0)),
                (3, Matrix4::IDENTITY),
            ]
        );
    }
}
