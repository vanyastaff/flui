//! [`Tester`]: one test over a [`Connection`] (ADR-0400 §1).

use std::collections::VecDeque;
use std::panic::Location;
use std::time::Duration;

use flui_protocol::act::{ActionabilityCheck, ActionabilityParams};
use flui_protocol::client::{CallError, Connection, EndChecks};
use flui_protocol::error::ErrorCode;
use flui_protocol::geometry::LogicalRect;
use flui_protocol::input::{
    Direction, DragParams, GestureOptions, LongPressParams, PressKeysParams, ScrollIntoViewParams,
    ScrollUntilParams, SwipeParams, TapParams, TypeTextParams,
};
use flui_protocol::locator::{LocateParams, Locator, Match, STRICT_CANDIDATES, resolve_strict};
use flui_protocol::method::{
    ElementActionability, ElementDragTo, ElementFling, ElementLongPress, ElementPressKeys,
    ElementScrollIntoView, ElementScrollUntil, ElementTap, ElementTypeText, SemanticsLocate,
    SemanticsRead, WindowMethod,
};
use flui_protocol::tree::{Node, ReadParams, Tree, WindowId, outline};
use flui_protocol::wait::{Condition, SettleScope, UnsettledCause, WaitEnd};
use flui_view::IntoView;

use super::clock::VirtualClock;
use super::error::{Diagnosis, OrRaise as _, TestError};
use super::find::{Find, Many, One, Quantity};
use super::headless::{FRAME, Headless, Observed, SETTLE_LIMIT};
use super::matcher::Matcher;
use super::report::{Exit, Report, Running, TestReport, write_report};
use super::target::Target;
use super::window::TestWindow;
use crate::drop_ledger::DropLedger;

/// How long an expectation holds out by default, on the connection's clock.
pub const EXPECT_LIMIT: Duration = Duration::from_secs(5);

/// Outline lines a failure lists.
const SHOWN_LINES: usize = 15;

/// Candidates a failure to find lists.
const NEAREST: usize = 5;

/// Journal steps a failure lists, the latest last.
const JOURNAL_KEPT: usize = 32;

/// One test of an application over a connection (ADR-0400 §1).
///
/// Over a [`Connection`], its verbs find with a [`Find`], act
/// through the window's agent and expect with a [`Matcher`], so each is evaluated once, in the
/// application's runtime, on every connection. Every verb has a `try_` twin returning
/// [`TestError`]; the plain form panics at its caller. The tester keeps no copy of the
/// application's state: what is settled is the host's, the input held between steps is the
/// agent's lease, and the wait loop is the connection's. A tester over
/// [`Headless<Unobserved>`](super::Unobserved), which has no agent, pumps, reads its frame and
/// measures only.
///
/// ```
/// use flui_testing::scenario::{Tester, find, matcher::exists};
/// use flui_widgets::{Center, Text};
///
/// let mut t = Tester::mount(Center::new().child(Text::new("Hello")));
/// t.expect(&find::text("Hello"), exists());
/// ```
///
/// # The end of a test
///
/// The tester's report owns the first failure, the soft failures, whether a call may have run
/// and the errors its `try_` verbs returned that are still alive (ADR-0400 §7). [`finish`]
/// and `Drop` run the end checks when the report is not poisoned (no verb unwound, no call went
/// unanswered) and the thread is not unwinding, and `Drop` only when no returned error is alive
/// (it may be on its way to the caller through `?`; a test that calls `finish` says it is
/// done, an error it kept included): the application is
/// settled, its frame audited, its root unmounted, its input lease released (on an unobserved
/// target, which has no agent, the end runs neither the audit nor a release), the tester's
/// [`ledger`](Tester::ledger) read, and what keeps the application busy after that (a timer, a
/// ticker or background work left behind) fails the end. `finish` raises what did not pass;
/// `Drop` writes it to the test's output and raises nothing. Otherwise the end releases and
/// checks nothing, since the first failure is already on its way to the caller.
///
/// [`finish`]: Tester::finish
pub struct Tester<C: Target = Headless> {
    connection: C,
    window: WindowId,
    report: Report,
    journal: Journal,
    /// The options the next gesture takes ([`Tester::with`]).
    options: Option<GestureOptions>,
    /// Whether the end ran: [`Tester::finish`] or its twins.
    ended: bool,
}

impl<C: Target> std::fmt::Debug for Tester<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tester")
            .field("window", &self.window)
            .field("steps", &self.journal.next)
            .finish_non_exhaustive()
    }
}

/// One step of a tester's journal.
pub(crate) struct Step {
    number: u64,
    verb: &'static str,
    caller: &'static Location<'static>,
}

/// The steps a tester ran, the latest [`JOURNAL_KEPT`] of them, each with its caller.
#[derive(Default)]
struct Journal {
    /// Each step's number and its line.
    steps: VecDeque<(u64, String)>,
    next: u64,
}

impl Journal {
    fn begin(&mut self, verb: &'static str, caller: &'static Location<'static>) -> Step {
        self.next = self.next.saturating_add(1);
        if self.steps.len() == JOURNAL_KEPT {
            self.steps.pop_front();
        }
        self.steps
            .push_back((self.next, format!("{} `{verb}` at {caller}", self.next)));
        Step {
            number: self.next,
            verb,
            caller,
        }
    }

    /// The steps kept before `step`, the latest last.
    fn before(&self, step: &Step) -> Vec<String> {
        self.steps
            .iter()
            .filter(|(number, _)| *number < step.number)
            .map(|(_, line)| line.clone())
            .collect::<Vec<String>>()
    }
}

impl Tester<Headless<Observed>> {
    /// Mount `view` in an 800 by 600 window, as `run_app` mounts it: the window is handed to its
    /// agent and two frames are presented, so the first read finds a tree.
    ///
    /// # Panics
    ///
    /// When a first frame fails.
    #[track_caller]
    #[must_use]
    pub fn mount(view: impl IntoView) -> Self {
        Self::mount_in(view, TestWindow::default())
    }

    /// Mount `view` in `window`, as [`mount`](Self::mount) does.
    ///
    /// # Panics
    ///
    /// When a registration of `window` conflicts with another, or a first frame fails.
    #[track_caller]
    #[must_use]
    pub fn mount_in(view: impl IntoView, window: TestWindow) -> Self {
        let connection = Headless::mount(view, &window);
        let id = connection.window_id();
        Self::over(connection, id)
    }
}

impl<S> Tester<Headless<S>>
where
    Headless<S>: Target,
{
    /// The ledger the end of the test reads: plant a [`DropSentinel`](crate::drop_ledger::DropSentinel)
    /// from it in each value the test hands the application, and the end fails while one is
    /// alive once the root is unmounted and what it held released.
    #[must_use]
    pub fn ledger(&self) -> &DropLedger {
        self.connection.ledger()
    }
}

impl<C: Target> Tester<C> {
    pub(super) fn over(connection: C, window: WindowId) -> Self {
        Self {
            connection,
            window,
            report: Report::default(),
            journal: Journal::default(),
            options: None,
            ended: false,
        }
    }

    /// The connection, for what only its target has.
    #[must_use]
    pub fn connection(&self) -> &C {
        &self.connection
    }

    /// The connection, for what only its target has: a call the verbs do not make, an
    /// [`ActionSequence`](flui_protocol::input::ActionSequence) performed as it is.
    pub fn connection_mut(&mut self) -> &mut C {
        &mut self.connection
    }

    /// The window the tester drives.
    #[must_use]
    pub fn window(&self) -> WindowId {
        self.window
    }

    fn step(&mut self, verb: &'static str, caller: &'static Location<'static>) -> Step {
        self.journal.begin(verb, caller)
    }

    /// A step of the journal, and the guard that poisons the report should it unwind: what
    /// every read of the tester's target begins with.
    pub(crate) fn begin(
        &mut self,
        verb: &'static str,
        caller: &'static Location<'static>,
    ) -> (Step, Running) {
        let step = self.step(verb, caller);
        let running = self.report.guard(step.verb, step.caller);
        (step, running)
    }

    /// The step's error for a call of the target that failed and names no find: an unanswered
    /// call poisons the tester.
    pub(super) fn failed(&mut self, step: &Step, error: CallError) -> TestError {
        match error {
            CallError::Unanswered {
                may_have_run,
                released,
            } => {
                self.report.unanswered(step.verb, may_have_run);
                TestError::Unanswered {
                    may_have_run,
                    released,
                    diagnosis: Box::new(self.diagnosis(step, Vec::new(), Vec::new())),
                }
            }
            CallError::Failed(object) => TestError::Agent {
                diagnosis: Box::new(self.diagnosis(step, Vec::new(), Vec::new())),
                error: object,
            },
            other => TestError::Agent {
                error: Box::new(flui_protocol::error::ErrorObject::new(
                    ErrorCode::Platform,
                    other.to_string(),
                )),
                diagnosis: Box::new(self.diagnosis(step, Vec::new(), Vec::new())),
            },
        }
    }
}

impl<C: Connection> Tester<C> {
    /// The protocol's options for the next gesture only: `force`, which skips every
    /// actionability check but `attached`, the act's timeout, the input path.
    pub fn with(&mut self, options: GestureOptions) -> &mut Self {
        self.options = Some(options);
        self
    }

    fn take_options(&mut self) -> GestureOptions {
        self.options.take().unwrap_or_default()
    }

    // --- Reading ----------------------------------------------------------

    /// The one element `find` names, as the semantics tree publishes it. A read is a turn with
    /// no frame: it reads the frame the window last presented.
    ///
    /// # Panics
    ///
    /// As [`Self::try_read`] fails.
    #[track_caller]
    #[must_use]
    pub fn read(&mut self, find: &Find<One>) -> Node {
        self.try_read(find).or_raise()
    }

    /// The one element `find` names.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`] or [`TestError::Ambiguous`] unless exactly one element matches
    /// (or the find picks one); [`TestError::Agent`] when the window cannot answer.
    #[track_caller]
    pub fn try_read(&mut self, find: &Find<One>) -> Result<Node, TestError> {
        let step = self.step("read", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        Ok(self.resolve(&step, find)?.node)
    }

    /// Every element `find` matches now, in tree order.
    ///
    /// # Panics
    ///
    /// As [`Self::try_find_all`] fails.
    #[track_caller]
    #[must_use]
    pub fn find_all(&mut self, find: &Find<Many>) -> Vec<Match> {
        self.try_find_all(find).or_raise()
    }

    /// Every element `find` matches now, in tree order.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window cannot answer.
    #[track_caller]
    pub fn try_find_all(&mut self, find: &Find<Many>) -> Result<Vec<Match>, TestError> {
        let step = self.step("find_all", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        Ok(self.locate(&step, find.locator(), None)?.matches)
    }

    /// The bounds of the one element `find` names, in the window's logical pixels.
    ///
    /// # Panics
    ///
    /// As [`Self::try_rect`] fails.
    #[track_caller]
    #[must_use]
    pub fn rect(&mut self, find: &Find<One>) -> LogicalRect {
        self.try_rect(find).or_raise()
    }

    /// The bounds of the one element `find` names.
    ///
    /// # Errors
    ///
    /// As [`Self::try_read`], and [`TestError::NotActionable`] (`visible`) for an element with
    /// no bounds.
    #[track_caller]
    pub fn try_rect(&mut self, find: &Find<One>) -> Result<LogicalRect, TestError> {
        let step = self.step("rect", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        let found = self.resolve(&step, find)?;
        let facts = self
            .connection
            .call::<ElementActionability>(ActionabilityParams::new(found.element))
            .map_err(|error| self.call_error(&step, Some(find.locator()), error))?;
        match facts.rect {
            Some(rect) => Ok(rect),
            None => Err(TestError::NotActionable {
                find: Box::new(find.locator().clone()),
                check: Some(ActionabilityCheck::Visible),
                covered_by: None,
                diagnosis: Box::new(self.diagnosis(&step, Vec::new(), Vec::new())),
            }),
        }
    }

    /// The window's semantics tree, as an agent reads it.
    ///
    /// # Panics
    ///
    /// As [`Self::try_snapshot`] fails.
    #[track_caller]
    #[must_use]
    pub fn snapshot(&mut self) -> Tree {
        self.try_snapshot().or_raise()
    }

    /// The window's semantics tree.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window cannot answer.
    #[track_caller]
    pub fn try_snapshot(&mut self) -> Result<Tree, TestError> {
        let step = self.step("snapshot", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        self.connection
            .call::<SemanticsRead>(ReadParams::new(self.window))
            .map_err(|error| self.call_error(&step, None, error))
    }

    // --- Acting -----------------------------------------------------------

    /// Tap the one element `find` names, once the agent finds it actionable: attached, visible,
    /// still over two frames, enabled and receiving the hit.
    ///
    /// # Panics
    ///
    /// As [`Self::try_tap`] fails.
    #[track_caller]
    pub fn tap(&mut self, find: &Find<One>) {
        if let Err(error) = self.try_tap(find) {
            error.raise();
        }
    }

    /// Tap the one element `find` names.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`], [`TestError::Ambiguous`] or [`TestError::NotActionable`], and
    /// nothing was sent; [`TestError::Agent`] for another refusal.
    #[track_caller]
    pub fn try_tap(&mut self, find: &Find<One>) -> Result<(), TestError> {
        let step = self.step("tap", Location::caller());
        let mut params = TapParams::new(find.locator().clone());
        params.options = self.take_options();
        self.act::<ElementTap>(&step, find.locator(), params)
    }

    /// Press the one element `find` names for the platform's long-press delay, then release.
    ///
    /// # Panics
    ///
    /// As [`Self::try_long_press`] fails.
    #[track_caller]
    pub fn long_press(&mut self, find: &Find<One>) {
        if let Err(error) = self.try_long_press(find) {
            error.raise();
        }
    }

    /// Press the one element `find` names for the platform's long-press delay.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    #[track_caller]
    pub fn try_long_press(&mut self, find: &Find<One>) -> Result<(), TestError> {
        let step = self.step("long_press", Location::caller());
        let mut params = LongPressParams::new(find.locator().clone());
        params.options = self.take_options();
        self.act::<ElementLongPress>(&step, find.locator(), params)
    }

    /// Pick up the one element `find` names and drop it on the one `to` names.
    ///
    /// # Panics
    ///
    /// As [`Self::try_drag`] fails.
    #[track_caller]
    pub fn drag(&mut self, find: &Find<One>, to: &Find<One>) {
        if let Err(error) = self.try_drag(find, to) {
            error.raise();
        }
    }

    /// Pick up the one element `find` names and drop it on the one `to` names.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    #[track_caller]
    pub fn try_drag(&mut self, find: &Find<One>, to: &Find<One>) -> Result<(), TestError> {
        let step = self.step("drag", Location::caller());
        let mut params = DragParams::new(find.locator().clone(), to.locator().clone());
        params.options = self.take_options();
        self.act::<ElementDragTo>(&step, find.locator(), params)
    }

    /// Fling the one element `find` names towards `direction`: a stroke that ends above the
    /// platform's fling velocity.
    ///
    /// # Panics
    ///
    /// As [`Self::try_fling`] fails.
    #[track_caller]
    pub fn fling(&mut self, find: &Find<One>, direction: Direction) {
        if let Err(error) = self.try_fling(find, direction) {
            error.raise();
        }
    }

    /// Fling the one element `find` names towards `direction`.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    #[track_caller]
    pub fn try_fling(&mut self, find: &Find<One>, direction: Direction) -> Result<(), TestError> {
        let step = self.step("fling", Location::caller());
        let mut params = SwipeParams::new(find.locator().clone(), direction);
        params.options = self.take_options();
        self.act::<ElementFling>(&step, find.locator(), params)
    }

    /// Scroll each scrollable ancestor of the one element `find` names until it is visible.
    ///
    /// # Panics
    ///
    /// As [`Self::try_scroll_into_view`] fails.
    #[track_caller]
    pub fn scroll_into_view(&mut self, find: &Find<One>) {
        if let Err(error) = self.try_scroll_into_view(find) {
            error.raise();
        }
    }

    /// Scroll the one element `find` names into view.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    #[track_caller]
    pub fn try_scroll_into_view(&mut self, find: &Find<One>) -> Result<(), TestError> {
        let step = self.step("scroll_into_view", Location::caller());
        let mut params = ScrollIntoViewParams::new(find.locator().clone());
        params.options = self.take_options();
        self.act::<ElementScrollIntoView>(&step, find.locator(), params)
    }

    /// Scroll the one scrollable `scrollable` names a viewport at a time, at frame boundaries,
    /// until `find` resolves inside it or its scroll extent ends.
    ///
    /// # Panics
    ///
    /// As [`Self::try_scroll_until_visible`] fails.
    #[track_caller]
    pub fn scroll_until_visible(&mut self, scrollable: &Find<One>, find: &Find<One>) {
        if let Err(error) = self.try_scroll_until_visible(scrollable, find) {
            error.raise();
        }
    }

    /// Scroll `scrollable` until `find` resolves inside it.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`], for the scrollable and for what it scrolls to.
    #[track_caller]
    pub fn try_scroll_until_visible(
        &mut self,
        scrollable: &Find<One>,
        find: &Find<One>,
    ) -> Result<(), TestError> {
        let step = self.step("scroll_until_visible", Location::caller());
        let mut params =
            ScrollUntilParams::new(scrollable.locator().clone(), find.locator().clone());
        params.options = self.take_options();
        self.act::<ElementScrollUntil>(&step, find.locator(), params)
    }

    /// Focus the one field `find` names, then commit `text` into it as an input method does.
    ///
    /// # Panics
    ///
    /// As [`Self::try_type_text`] fails.
    #[track_caller]
    pub fn type_text(&mut self, find: &Find<One>, text: &str) {
        if let Err(error) = self.try_type_text(find, text) {
            error.raise();
        }
    }

    /// Focus the one field `find` names, then commit `text` into it.
    ///
    /// # Errors
    ///
    /// As [`Self::try_tap`].
    #[track_caller]
    pub fn try_type_text(&mut self, find: &Find<One>, text: &str) -> Result<(), TestError> {
        let step = self.step("type_text", Location::caller());
        let mut params = TypeTextParams::new(find.locator().clone(), text);
        params.options = self.take_options();
        self.act::<ElementTypeText>(&step, find.locator(), params)
    }

    /// Press `chords` in turn to the window's focused element: `"Enter"`, `"Control+a"`,
    /// `"Shift+Tab"`. A chord is read as the protocol reads it
    /// ([`chord_keys`](flui_protocol::input::gestures::chord_keys)), presses its keys left to
    /// right and releases them right to left. A key the window's input lease already holds, as an
    /// earlier step left it down, is neither pressed again nor released: the lease is the one
    /// owner of held input.
    ///
    /// # Panics
    ///
    /// As [`Self::try_press_keys`] fails.
    #[track_caller]
    pub fn press_keys(&mut self, chords: &[&str]) {
        if let Err(error) = self.try_press_keys(chords) {
            error.raise();
        }
    }

    /// Press `chords` in turn to the window's focused element.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] (`invalid_argument`) for a chord that does not read, and nothing is
    /// pressed then.
    #[track_caller]
    pub fn try_press_keys(&mut self, chords: &[&str]) -> Result<(), TestError> {
        let step = self.step("press_keys", Location::caller());
        let keys = chords
            .iter()
            .map(|chord| (*chord).to_owned())
            .collect::<Vec<String>>();
        let mut params = PressKeysParams::new(self.window, keys);
        params.options = self.take_options();
        let _running = self.report.guard(step.verb, step.caller);
        self.connection
            .call::<ElementPressKeys>(params)
            .map(|_outcome| ())
            .map_err(|error| self.call_error(&step, None, error))
    }

    // --- Expecting --------------------------------------------------------

    /// Hold out until `matcher` holds of what `find` names, at most [`EXPECT_LIMIT`] of the
    /// connection's clock. The condition is evaluated by the application, at frame boundaries
    /// and after input; it holds once it is first true.
    ///
    /// # Panics
    ///
    /// As [`Self::try_expect`] fails.
    #[track_caller]
    pub fn expect<Q: Quantity>(&mut self, find: &Find<Q>, matcher: Matcher) {
        if let Err(error) = self.try_expect(find, matcher) {
            error.raise();
        }
    }

    /// Hold out until `matcher` holds of what `find` names.
    ///
    /// # Errors
    ///
    /// [`TestError::Unsatisfied`] when the condition did not hold within the limit, or the
    /// application went idle with it false, naming what kept it busy.
    #[track_caller]
    pub fn try_expect<Q: Quantity>(
        &mut self,
        find: &Find<Q>,
        matcher: Matcher,
    ) -> Result<(), TestError> {
        let step = self.step("expect", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        let condition = matcher.condition(find);
        let outcome = self
            .connection
            .wait(&condition, EXPECT_LIMIT)
            .map_err(|error| self.call_error(&step, Some(find.locator()), error))?;
        if outcome.end == WaitEnd::Satisfied {
            return Ok(());
        }
        let seen = outcome
            .observation
            .matched
            .iter()
            .take(NEAREST)
            .map(line_of)
            .collect::<Vec<String>>();
        Err(TestError::Unsatisfied {
            condition: Box::new(condition),
            end: outcome.end,
            observation: Box::new(outcome.observation),
            diagnosis: Box::new(self.diagnosis(&step, seen, outcome.unsettled)),
        })
    }

    /// Hold out until the application settles, at most [`EXPECT_LIMIT`]: on any connection, the
    /// frame it was answered on and which halves of the causes the answer counted.
    ///
    /// # Panics
    ///
    /// As [`Self::try_wait_settled`] fails.
    #[track_caller]
    pub fn wait_settled(&mut self) -> SettledAt {
        self.try_wait_settled().or_raise()
    }

    /// Hold out until the application settles.
    ///
    /// # Errors
    ///
    /// [`TestError::NotSettled`] naming every cause still holding it.
    #[track_caller]
    pub fn try_wait_settled(&mut self) -> Result<SettledAt, TestError> {
        let step = self.step("wait_settled", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        self.settled(&step, EXPECT_LIMIT)
    }

    fn settled(&mut self, step: &Step, limit: Duration) -> Result<SettledAt, TestError> {
        let outcome = self
            .connection
            .wait(&Condition::Settled, limit)
            .map_err(|error| self.call_error(step, None, error))?;
        if outcome.end == WaitEnd::Satisfied {
            return Ok(SettledAt {
                frame: self.connection.now().frame,
                scope: outcome.scope,
            });
        }
        Err(TestError::NotSettled {
            limit,
            diagnosis: Box::new(self.diagnosis(step, Vec::new(), outcome.unsettled)),
        })
    }

    /// Run `body` with soft expectations: each failure is kept in the report instead of
    /// raised, and the list is raised once when `body` returns, then cleared. A soft failure a
    /// panic in `body` leaves unraised is written to the test's output at its end.
    ///
    /// # Panics
    ///
    /// When a soft expectation failed, listing them all.
    #[track_caller]
    pub fn checks<R>(&mut self, body: impl FnOnce(&mut Checks<'_, C>) -> R) -> R {
        let caller = Location::caller();
        let step = self.step("checks", caller);
        let _running = self.report.guard(step.verb, step.caller);
        let result = body(&mut Checks { tester: self });
        let soft = self.report.take_soft();
        if !soft.is_empty() {
            let message = format!(
                "{} soft failure(s) in the checks at {caller}:\n- {}",
                soft.len(),
                soft.join("\n- ")
            );
            self.report.raised(&message);
            panic!("{message}");
        }
        result
    }
}

impl<C: Target> Tester<C> {
    // --- The end ----------------------------------------------------------

    /// End the test, running its end checks (see [the type's docs](Tester#the-end-of-a-test)).
    ///
    /// # Panics
    ///
    /// With the [`TestReport`] when an end check did not pass, a soft failure was never raised,
    /// or the checks could not run.
    #[track_caller]
    pub fn finish(self) {
        if let Err(report) = self.try_finish() {
            panic!("{report}");
        }
    }

    /// End the test, running its end checks.
    ///
    /// # Errors
    ///
    /// The [`TestReport`] unless it is clean.
    pub fn try_finish(mut self) -> Result<(), TestReport> {
        let report = self.end(Exit::Finish);
        if report.is_clean() {
            Ok(())
        } else {
            Err(report)
        }
    }

    /// End the test without its checks, for the stated `reason`: the connection releases what it
    /// holds and nothing is read. A soft failure a panic in [`Self::checks`] left unraised is
    /// written to the test's output, as the end of a dropped tester writes it.
    pub fn finish_unchecked(mut self, reason: &str) {
        self.ended = true;
        tracing::debug!(reason, "a test ended without its end checks");
        let soft = self.report.take_soft();
        let _unpassed = self.connection.end(&EndChecks::none());
        write_report(&TestReport {
            unpassed: Vec::new(),
            soft,
            skipped: None,
        });
    }

    /// The end of the test (ADR-0400 §7): the checks when the report lets them run, else the
    /// release alone, unless the thread is unwinding, when the fields' own drops release.
    fn end(&mut self, exit: Exit) -> TestReport {
        self.ended = true;
        let soft = self.report.take_soft();
        if let Some(skipped) = self.report.skips_end_checks(exit) {
            if !std::thread::panicking() {
                let _unpassed = self.connection.end(&EndChecks::none());
            }
            return TestReport {
                unpassed: Vec::new(),
                soft,
                skipped: Some(skipped),
            };
        }
        let unpassed = self.connection.end(&EndChecks::default());
        TestReport {
            unpassed,
            soft,
            skipped: None,
        }
    }
}

impl<C: Connection> Tester<C> {
    // --- Internals --------------------------------------------------------

    /// Send one gesture: what the agent refused is the step's error.
    fn act<M: WindowMethod>(
        &mut self,
        step: &Step,
        find: &Locator,
        params: M::Params,
    ) -> Result<(), TestError> {
        let _running = self.report.guard(step.verb, step.caller);
        self.connection
            .call::<M>(params)
            .map(|_outcome| ())
            .map_err(|error| self.call_error(step, Some(find), error))
    }

    /// The matches of `locator`, at most `max_matches` of them reported and the rest counted.
    fn locate(
        &mut self,
        step: &Step,
        locator: &Locator,
        max_matches: Option<u32>,
    ) -> Result<flui_protocol::locator::Matches, TestError> {
        let mut params = LocateParams::new(locator.clone());
        params.max_matches = max_matches;
        self.connection
            .call::<SemanticsLocate>(params)
            .map_err(|error| self.call_error(step, Some(locator), error))
    }

    /// The one match of `find`, or why there is not one. The locate reports at most the
    /// [`STRICT_CANDIDATES`] an ambiguity names and counts the rest.
    pub(crate) fn resolve(&mut self, step: &Step, find: &Find<One>) -> Result<Match, TestError> {
        let matches = self.locate(step, find.locator(), Some(STRICT_CANDIDATES))?;
        match resolve_strict(matches.matches.iter(), matches.truncated) {
            Ok(Some(found)) => Ok(found.clone()),
            Ok(None) => Err(self.not_found(step, find.locator())),
            Err(ambiguity) => Err(TestError::Ambiguous {
                find: Box::new(find.locator().clone()),
                matched: u32::try_from(ambiguity.matched).unwrap_or(u32::MAX),
                diagnosis: Box::new(self.diagnosis(step, ambiguity.candidates, Vec::new())),
            }),
        }
    }

    fn not_found(&mut self, step: &Step, find: &Locator) -> TestError {
        let tree = self
            .connection
            .call::<SemanticsRead>(ReadParams::new(self.window))
            .ok();
        let nearest = tree.map_or_else(Vec::new, |tree| nearest(find, &tree));
        TestError::NotFound {
            find: Box::new(find.clone()),
            diagnosis: Box::new(self.diagnosis(step, nearest, Vec::new())),
        }
    }

    /// The step's error for a call that failed: the agent's refusals of a find become their
    /// own variants, an unanswered call poisons the tester.
    fn call_error(&mut self, step: &Step, find: Option<&Locator>, error: CallError) -> TestError {
        match error {
            CallError::Failed(object) => match (object.code, find) {
                (ErrorCode::NotFound, Some(find)) => self.not_found(step, find),
                (ErrorCode::Ambiguous, Some(find)) => {
                    let matched = self
                        .connection
                        .call::<SemanticsLocate>(LocateParams::new(find.clone()))
                        .map_or(0, |matches| {
                            matches.matches.len().saturating_add(
                                usize::try_from(matches.truncated).unwrap_or(usize::MAX),
                            )
                        });
                    TestError::Ambiguous {
                        find: Box::new(find.clone()),
                        matched: u32::try_from(matched).unwrap_or(u32::MAX),
                        diagnosis: Box::new(self.diagnosis(
                            step,
                            object.candidates.clone(),
                            Vec::new(),
                        )),
                    }
                }
                (ErrorCode::NotActionable, Some(find)) => TestError::NotActionable {
                    find: Box::new(find.clone()),
                    check: object.failed,
                    covered_by: object.covered_by.clone(),
                    diagnosis: Box::new(self.diagnosis(step, Vec::new(), Vec::new())),
                },
                _ => self.failed(step, CallError::Failed(object)),
            },
            other => self.failed(step, other),
        }
    }
}

impl<C: Target> Tester<C> {
    /// The diagnosis of `step`, registered with the report: the window's outline is read now, a
    /// turn with no frame.
    pub(crate) fn diagnosis(
        &mut self,
        step: &Step,
        candidates: Vec<String>,
        unsettled: Vec<UnsettledCause>,
    ) -> Diagnosis {
        let shown = self.connection.outline(self.window);
        let reading = self.connection.reading();
        Diagnosis {
            step: step.number,
            verb: step.verb,
            caller: step.caller,
            frame: reading.frame,
            time: reading.time,
            candidates,
            outline: shown,
            unsettled,
            journal: self.journal.before(step),
            registration: Some(self.report.register()),
        }
    }
}

impl<C: Target + VirtualClock> Tester<C> {
    /// Run one frame, [`FRAME`] after the last.
    ///
    /// # Panics
    ///
    /// With the first failure the frame raised.
    #[track_caller]
    pub fn pump(&mut self) {
        self.advance("pump", Location::caller(), FRAME);
    }

    /// Move the clock by `by`, in frames of [`FRAME`], the last one shorter when `by` is not a
    /// whole number of frames.
    ///
    /// # Panics
    ///
    /// With the first failure a frame raised.
    #[track_caller]
    pub fn pump_for(&mut self, by: Duration) {
        self.advance("pump_for", Location::caller(), by);
    }

    #[track_caller]
    fn advance(&mut self, verb: &'static str, caller: &'static Location<'static>, by: Duration) {
        let step = self.step(verb, caller);
        let _running = self.report.guard(step.verb, step.caller);
        if let Err(error) = self.connection.advance(by) {
            self.failed(&step, error).raise();
        }
    }
}

impl<C: Connection + VirtualClock> Tester<C> {
    /// Run the application until nothing keeps it busy (ADR-0400 §3), on the virtual clock, at
    /// most five seconds of it: the frame it gives is still, the only one a frame-exact check
    /// reads.
    ///
    /// # Panics
    ///
    /// As [`Self::try_settle`] fails.
    #[track_caller]
    pub fn settle(&mut self) -> Settled<'_, C> {
        match self.try_settle() {
            Ok(settled) => settled,
            Err(error) => error.raise(),
        }
    }

    /// Run the application until nothing keeps it busy.
    ///
    /// # Errors
    ///
    /// [`TestError::NotSettled`] naming every cause still holding it, a repeating animation
    /// among them: [`Self::freeze_at`] holds such a frame instead.
    #[track_caller]
    pub fn try_settle(&mut self) -> Result<Settled<'_, C>, TestError> {
        let step = self.step("settle", Location::caller());
        let _running = self.report.guard(step.verb, step.caller);
        let at = self.settled(&step, SETTLE_LIMIT)?;
        Ok(Settled {
            frame: at.frame,
            running: Vec::new(),
            _tester: self,
        })
    }

    /// Run the application for `after`, then hold the frame at that instant with the causes
    /// still running named: a screen with a repeating animation has a still frame at a stated
    /// instant, and no switch in the widget.
    ///
    /// # Panics
    ///
    /// With the first failure a frame raised.
    #[track_caller]
    pub fn freeze_at(&mut self, after: Duration) -> Settled<'_, C> {
        let caller = Location::caller();
        self.advance("freeze_at", caller, after);
        let step = self.step("freeze_at", caller);
        let _running = self.report.guard(step.verb, step.caller);
        let running = match self.connection.wait(&Condition::Settled, Duration::ZERO) {
            Ok(outcome) => outcome.unsettled,
            Err(error) => self.call_error(&step, None, error).raise(),
        };
        Settled {
            frame: self.connection.now().frame,
            running,
            _tester: self,
        }
    }
}

impl<C: Target> Drop for Tester<C> {
    /// The end of the test in report-only mode: the end checks run as [`Tester::finish`] runs
    /// them, and what did not pass is written to the test's output, never raised.
    fn drop(&mut self) {
        if self.ended {
            return;
        }
        let report = self.end(Exit::Drop);
        write_report(&report);
    }
}

/// Soft expectations inside [`Tester::checks`].
pub struct Checks<'t, C: Connection> {
    tester: &'t mut Tester<C>,
}

impl<C: Connection> std::fmt::Debug for Checks<'_, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checks").finish_non_exhaustive()
    }
}

impl<C: Connection> Checks<'_, C> {
    /// [`Tester::expect`], kept as a soft failure instead of raised.
    #[track_caller]
    pub fn expect<Q: Quantity>(&mut self, find: &Find<Q>, matcher: Matcher) {
        if let Err(error) = self.tester.try_expect(find, matcher) {
            self.tester.report.soft(error.to_string());
        }
    }

    /// The tester, to act between soft expectations.
    pub fn tester(&mut self) -> &mut Tester<C> {
        self.tester
    }
}

/// The frame a wait for settledness was answered on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettledAt {
    /// The frame the window last presented, when the connection knows it.
    pub frame: Option<u64>,
    /// Which halves of the causes the answer counted.
    pub scope: SettleScope,
}

/// A frame that does not move: what [`Tester::settle`] and [`Tester::freeze_at`] give, and
/// nothing acts or pumps while it lives (ADR-0400 §6).
pub struct Settled<'t, C: Connection> {
    frame: Option<u64>,
    running: Vec<UnsettledCause>,
    _tester: &'t mut Tester<C>,
}

impl<C: Connection> std::fmt::Debug for Settled<'_, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settled")
            .field("frame", &self.frame)
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

impl<C: Connection> Settled<'_, C> {
    /// The frame held, when the connection knows its number.
    #[must_use]
    pub fn frame(&self) -> Option<u64> {
        self.frame
    }

    /// What still runs at the frame held: empty after a settle, the repeating causes after
    /// [`Tester::freeze_at`].
    #[must_use]
    pub fn running(&self) -> &[UnsettledCause] {
        &self.running
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

/// The window's outline, cut to its first lines.
pub(super) fn excerpt(tree: &Tree) -> String {
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

/// Whether a name and the text a locator looks for share text: the name holds the text, or the
/// text holds the name as one of its words, so "Save" finds "Save draft" and "Save draft" finds
/// "Save", and a one-letter name is not nearest to every text that contains the letter.
fn shares_text(needle: &str, name: &str) -> bool {
    name.contains(needle) || needle.split_whitespace().any(|word| word == name)
}

/// The published nodes nearest what `find` looks for: its identifier, a name sharing its text
/// regardless of case, or its role.
fn nearest(find: &Locator, tree: &Tree) -> Vec<String> {
    // The text the locator compares names with, never its `Debug` form, whose field names would
    // match any short name.
    let needle = find
        .name
        .as_ref()
        .map(|name| name.needle().to_lowercase())
        .filter(|needle| !needle.is_empty());
    let mut found = Vec::new();
    let mut pending: Vec<&Node> = tree.roots.iter().collect();
    while let Some(node) = pending.pop() {
        pending.extend(node.children.iter().rev());
        let by_identifier = find
            .identifier
            .as_ref()
            .is_some_and(|identifier| node.identifier.as_ref() == Some(identifier));
        let by_name = node.name.as_ref().is_some_and(|name| {
            let name = name.to_lowercase();
            !name.is_empty()
                && needle
                    .as_ref()
                    .is_some_and(|needle| shares_text(needle, &name))
        });
        let by_role = find.role.is_some_and(|role| role == node.role);
        if by_identifier || by_name || by_role {
            found.push(line_of(node));
            if found.len() == NEAREST {
                break;
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use flui_protocol::locator::STRICT_CANDIDATES;
    use flui_view::{BoxedView, IntoView as _, ViewExt as _};
    use flui_widgets::{Column, Text};

    use super::super::find;
    use super::super::headless::Headless;
    use super::super::testing::RecordingConnection;
    use super::super::window::TestWindow;
    use super::{TestError, Tester};

    /// A strict read asks the window for no more matches than an ambiguity names and counts the
    /// rest from what the window left out; `find_all` asks for every match.
    #[test]
    fn a_strict_read_asks_for_no_more_matches_than_it_names() {
        let rows: Vec<BoxedView> = (0..12)
            .map(|_| Text::new("Row").into_view().boxed())
            .collect();
        let connection = Headless::mount(Column::new(rows), &TestWindow::default());
        let window = connection.window_id();
        let mut t = Tester::over(RecordingConnection::new(connection), window);
        let ambiguous = t.try_read(&find::text("Row"));
        assert!(
            matches!(ambiguous, Err(TestError::Ambiguous { matched: 12, .. })),
            "{ambiguous:?}"
        );
        drop(ambiguous);
        assert_eq!(t.find_all(&find::text("Row").all()).len(), 12);
        assert_eq!(
            t.connection().located(),
            [Some(STRICT_CANDIDATES), None],
            "the strict read is bounded, the read of all is not"
        );
        t.finish();
    }
}
