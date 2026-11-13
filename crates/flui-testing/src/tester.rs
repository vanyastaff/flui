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
//! proves is what assistive technology, the driver and an agent see.
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

use std::sync::Arc;
use std::time::Duration;

use flui_foundation::ManualClock;
use flui_protocol::method::{
    ElementActionability, InputInject, InspectRender, InspectWidgets, SemanticsLocate,
    SemanticsRead,
};
use flui_protocol::{
    ActionSequence, Actionability, ActionabilityCheck, ActionabilityParams, Checked, ErrorObject,
    InputParams, InputSource, InspectParams, InspectReading, LocateParams, Locator, LogicalPoint,
    LogicalRect, Match, Node, PointerAction, PointerButton, PointerDevice, ReadParams, Tree,
    outline,
};
use flui_view::View;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};
use parking_lot::Mutex;

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

/// At most this many zero-length frames run work due now before a read: a
/// woken completion, the rebuild it schedules and the semantics it commits.
const DUE_NOW_FRAMES: u32 = 8;

/// The window a [`Tester`] mounts its view in: its logical size, and the
/// platform whose conventions the application follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestWindow {
    width: u32,
    height: u32,
    platform: Option<flui_platform::TargetPlatform>,
}

impl TestWindow {
    /// A window of `width` by `height` logical pixels.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            platform: None,
        }
    }

    /// Follow `platform`'s conventions (chords, handles, menus, transitions)
    /// instead of the build host's.
    #[must_use]
    pub const fn platform(mut self, platform: flui_platform::TargetPlatform) -> Self {
        self.platform = Some(platform);
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
         semantics is found by an identifier: give it one with `Semantics::identifier` and find \
         it with `find::id`."
    )]
    NotFound {
        /// What was looked for.
        locator: Box<Locator>,
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
        let mut headless = HeadlessWindow::new(window.width, window.height).with_text_input();
        if let Some(platform) = window.platform {
            headless = headless.with_platform(platform);
        }
        let windows = Arc::new(Mutex::new(Vec::new()));
        let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
        let mut host = HeadlessHost::new(headless).with_dev_agent(&mut agent);
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
    pub fn clock(&self) -> &ManualClock {
        self.host.clock()
    }

    // --- Reading ----------------------------------------------------------

    /// Every element `locator` matches now, in tree order.
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
    /// element matches (or the locator picks one).
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
            0 => Err(TestError::NotFound {
                locator: Box::new(locator.clone()),
                shown: self.shown(),
            }),
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

    /// Deliver `sources` through the window's input lease and run the frame
    /// after the last tick.
    fn inject(&mut self, sources: Vec<InputSource>) -> Result<(), TestError> {
        // The agent plays a pause or a timed move for its whole duration
        // before it answers, so the budget is that span in frames on top of
        // the ticks' own.
        let span = sources.iter().map(source_span).max().unwrap_or_default();
        let frame_ms = u64::try_from(FRAME.as_millis()).unwrap_or(u64::MAX);
        let span_frames = u32::try_from(span.div_ceil(frame_ms)).unwrap_or(u32::MAX);
        let call = self.lease.call::<InputInject>(InputParams::new(
            self.lease.window(),
            ActionSequence::new(sources),
        ))?;
        let _outcome = self.answered_within(call, ANSWER_FRAMES.saturating_add(span_frames))?;
        Ok(())
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

    /// No element matches: `findsNothing`.
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

/// `node` as one outline line.
fn line_of(node: &Node) -> String {
    outline(std::slice::from_ref(node))
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// How long `source` plays in milliseconds: the sum of its pauses and timed
/// moves, saturating.
fn source_span(source: &InputSource) -> u64 {
    match source {
        InputSource::Pointer { actions, .. } => actions
            .iter()
            .map(|action| match action {
                PointerAction::Move { duration_ms, .. } | PointerAction::Pause { duration_ms } => {
                    *duration_ms
                }
                _ => 0,
            })
            .fold(0, u64::saturating_add),
        _ => 0,
    }
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
