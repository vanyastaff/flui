//! `element.scroll_until` answered in the application (ADR-0400 §2; Flutter's
//! `scrollUntilVisible`): the agent scrolls the scrollable a viewport at a time, at frame
//! boundaries, until the locator resolves to an element under it that shows, or the content stops
//! moving.
//!
//! Each step turns the wheel at the scrollable's visible centre by its extent along the
//! direction, as synthetic input through the call's lease, and the next attempt waits for that
//! injection's answer, which comes once a frame has shown what it did. The content is the
//! scrollable's descendants and their bounds: a step after which it is unchanged means the scroll
//! extent ended, or the scrollable does not scroll, and ends the search, so a scrollable that
//! does not move never spins. A call that names no direction scrolls down; a scrollable whose
//! content does not move on its first step down is tried rightwards.
//!
//! The act's deadline wakes the owner whether or not the window draws, and the pass at it is the
//! last: a step not played yet is withdrawn and one under way has sent its one wheel input, the
//! window as it is then is looked at once more, and the act answers. No scroll goes out after
//! the answer.
//!
//! A step reads at most [`CONTENT_NODES`] of the scrollable's descendants and looks among at
//! most [`MAX_MATCHES`] of the locator's matches in the window; a read that leaves some out is
//! refused rather than taken for the whole.

use flui_protocol::input::gestures;
use flui_protocol::method::{self, Method};
use flui_protocol::{
    act::ActOutcome, act::ActionabilityParams, act::Effect, act::EffectKind, act::JournalSpan,
    input::Direction, input::InputPath, input::ScrollUntilParams, locator::LocateParams,
    locator::Locator, locator::Match, locator::Pick, locator::Target, locator::resolve_strict,
    snapshot::Snapshot, tree::ElementId, tree::ElementRef, tree::Node, tree::ReadQuery, tree::Rect,
};
use flui_semantics::TreeSnapshot;
use flui_view::dev_agent::AgentAnswer;
use rustc_hash::FxHashSet;

use super::agent::{
    self, AgentError, IssuedHandles, ReadingOwed, ReplySender, record_issued_outcome,
};
use super::agent_gesture::{
    Aimed, Checks, Forms, Readiness, answer, deadline_of, readiness, takes_semantic_path, unleased,
};
use super::agent_held::{Attempt, AttemptContext, HeldAct, Holding};
use super::agent_input::{self, Injecting, InjectionId, widen};
use super::agent_serve::{Answer, Serve, ServeContext};

/// The most of a scrollable's descendants one step reads, to find the element under it and to
/// tell whether its content moved.
const CONTENT_NODES: usize = 4096;

/// How many of the window's matches of the locator one step looks among for one under the
/// scrollable.
const MAX_MATCHES: u32 = 4096;

/// A scrollable's content at one attempt: each descendant and its bounds, in tree order, and the
/// descendants as a set to look a match up in.
struct Content {
    shown: Vec<(ElementId, Option<Rect>)>,
    elements: FxHashSet<ElementId>,
}

impl Content {
    fn new(shown: Vec<(ElementId, Option<Rect>)>) -> Self {
        let elements = shown
            .iter()
            .map(|(element, _)| *element)
            .collect::<FxHashSet<ElementId>>();
        Self { shown, elements }
    }

    fn holds(&self, element: ElementId) -> bool {
        self.elements.contains(&element)
    }

    /// The same descendants at the same bounds: the content did not move.
    fn unmoved_since(&self, earlier: &Self) -> bool {
        self.shown == earlier.shown
    }
}

/// `element.scroll_until`, held while it scrolls.
pub(super) struct ScrollSearch {
    scrollable: Aimed,
    until: Locator,
    /// The way it scrolls now.
    direction: Direction,
    /// The call named no direction: a first step down that moves nothing turns it rightwards.
    may_turn: bool,
    /// The content has moved at least once.
    moved: bool,
    snapshot: Snapshot,
    /// The tree before the first step, for a delta reading.
    before: Option<TreeSnapshot>,
    /// The step under way.
    stepping: Option<Step>,
    /// The content when the last step was sent.
    sent_over: Option<Content>,
    /// The window's journal entries the steps made.
    observed: Option<JournalSpan>,
}

/// One step of a search: the injection that turns the wheel, and its answer.
struct Step {
    /// `None` when the window's queue refused it, which its answer says.
    injection: Option<InjectionId>,
    answer: AgentAnswer<ActOutcome>,
}

impl ScrollSearch {
    /// One attempt: answer once the element shows under the scrollable or the content stops
    /// moving, and scroll a step otherwise.
    pub(super) fn attempt(
        &mut self,
        ctx: &mut AttemptContext<'_>,
        reply: &ReplySender<ActOutcome>,
    ) -> Attempt {
        if let Some(step) = &mut self.stepping {
            let injection = step.injection;
            match step.answer.try_take() {
                // The step under way finishes before the next one is sent.
                None if !ctx.last => return Attempt::Hold,
                // The deadline passed first. A step not played yet is withdrawn; one under way
                // has sent its one wheel input at the boundary it started at. Nothing of it goes
                // out after this, and the window is looked at once more below.
                None => {
                    if let Some(injection) = injection {
                        agent_input::withdraw(ctx.state, injection);
                    }
                    self.stepping = None;
                }
                Some(Err(failed)) => {
                    self.stepping = None;
                    return answer(reply, Err(AgentError::InputFailed(Box::new(failed))));
                }
                Some(Ok(stepped)) => {
                    self.stepping = None;
                    self.observed = widen(self.observed, stepped.observed);
                }
            }
        }
        let (scrollable, aim) = match readiness(ctx, &self.scrollable, Checks::Attached) {
            Readiness::Ready(Some(scrollable), aim) => (scrollable, aim),
            Readiness::Ready(None, _) => return answer(reply, Err(names_no_scrollable())),
            Readiness::NotYet(why) if ctx.last => return answer(reply, Err(why)),
            Readiness::NotYet(_) => return Attempt::Hold,
            Readiness::Failed(error) => return answer(reply, Err(error)),
        };
        let content = match content_of(ctx, scrollable) {
            Ok(content) => content,
            Err(error) => return answer(reply, Err(error)),
        };
        match self.shown_under(ctx, &content) {
            Ok(Some(found)) => return self.found(ctx, reply, found),
            Ok(None) => {}
            Err(error) => return answer(reply, Err(error)),
        }
        if ctx.last {
            return answer(reply, Err(AgentError::NoMatch));
        }
        if let Some(sent_over) = &self.sent_over {
            if content.unmoved_since(sent_over) {
                if self.may_turn && !self.moved {
                    self.direction = Direction::Right;
                    self.may_turn = false;
                } else {
                    return answer(reply, Err(AgentError::ScrollEnded));
                }
            } else {
                self.moved = true;
            }
        }
        let Some(aim) = aim else {
            // Attached with no visible part: nothing to turn the wheel over yet.
            return Attempt::Hold;
        };
        let extent = aim.bounds.map_or(0.0, |bounds| match self.direction {
            Direction::Up | Direction::Down => bounds.height,
            _ => bounds.width,
        });
        if !(extent.is_finite() && extent > 0.0) {
            return Attempt::Hold;
        }
        let script = match agent_input::lower(&gestures::scroll(aim.point, self.direction, extent))
        {
            Ok(script) => script,
            Err(error) => return answer(reply, Err(error)),
        };
        let Some(lease) = ctx.lease else {
            return answer(reply, Err(unleased()));
        };
        let (step_reply, stepping) = flui_view::__runtime::reply_pair::<ActOutcome>();
        // A step's answer reports no element handle: the search's own answer does.
        let step_reply = ReplySender::new(
            step_reply,
            ctx.window,
            agent::issued_of(ctx.state),
            |_, _| {},
        );
        let injecting = Injecting {
            lease,
            script,
            element: Some(scrollable),
            snapshot: Snapshot::None,
            before: None,
            deadline: Some(ctx.deadline),
        };
        // Past here only the platform's wake runs, before anything is queued.
        ctx.if_panics.set(AgentError::Panicked);
        let injection = ctx
            .runtime
            .queue_injection(ctx.state, injecting, step_reply);
        self.stepping = Some(Step {
            injection,
            answer: stepping,
        });
        self.sent_over = Some(content);
        Attempt::Hold
    }

    /// The element the locator names under the scrollable, if it shows; `None` while it does not.
    fn shown_under(
        &self,
        ctx: &mut AttemptContext<'_>,
        content: &Content,
    ) -> Result<Option<ElementRef>, AgentError> {
        let mut every = self.until.clone();
        every.pick = Pick::Strict;
        let mut params = LocateParams::new(every);
        params.max_matches = Some(MAX_MATCHES);
        let matches = agent::locate(ctx.state, ctx.window, &params)?;
        if matches.truncated > 0 {
            // A match left out may be the one under the scrollable, or a second one there.
            return Err(AgentError::InvalidArgument {
                reason: "the locator matches more elements in the window than a scroll looks \
                         among (4096): narrow it",
            });
        }
        let under: Vec<&Match> = matches
            .matches
            .iter()
            .filter(|found| content.holds(found.element.element))
            .collect::<Vec<&Match>>();
        let picked = match self.until.pick {
            Pick::Strict => resolve_strict(under.iter().copied(), 0).map_err(AgentError::from)?,
            Pick::Last => under.last().copied(),
            Pick::Nth(index) => usize::try_from(index)
                .ok()
                .and_then(|index| under.get(index).copied()),
            _ => under.first().copied(),
        };
        let Some(found) = picked else {
            return Ok(None);
        };
        let element = found.element;
        let read = agent::actionability(
            ctx.state,
            ctx.window,
            &ActionabilityParams::new(element),
            ctx.caught,
        )?;
        Ok(read.reading.facts.visible.then_some(element))
    }

    /// The search ended at `found`: answer with it, and the reading asked for once a frame has
    /// passed.
    fn found(
        &mut self,
        ctx: &mut AttemptContext<'_>,
        reply: &ReplySender<ActOutcome>,
        found: ElementRef,
    ) -> Attempt {
        let mut outcome = ActOutcome::new(InputPath::Synthetic, Effect::new(EffectKind::Ran));
        outcome.element = Some(found);
        outcome.observed = self.observed;
        if self.snapshot == Snapshot::None {
            return answer(reply, Ok(outcome));
        }
        let owed = ReadingOwed::new(outcome, self.snapshot, self.before.take());
        ctx.runtime.park_act_read(
            ctx.state,
            owed,
            Some(ctx.deadline),
            reply.clone(),
            ctx.caught,
        );
        Attempt::Done
    }
}

/// The scrollable's descendants and their bounds in `ctx`'s committed tree.
fn content_of(ctx: &AttemptContext<'_>, scrollable: ElementRef) -> Result<Content, AgentError> {
    fn walk(nodes: &[Node], content: &mut Vec<(ElementId, Option<Rect>)>) {
        for node in nodes {
            content.push((node.id, node.surface_rect));
            walk(&node.children, content);
        }
    }
    let query = ReadQuery::new()
        .root(scrollable.element)
        .max_nodes(CONTENT_NODES);
    let tree = agent::read_tree(ctx.state, ctx.window, &query, true)?;
    if tree.truncated {
        // Neither whether a match is under it nor whether it moved can be told from a part.
        return Err(AgentError::NotSupported {
            capability: method::ElementScrollUntil::NAME,
            reason: "the scrollable holds more descendants than a scroll reads (4096)",
        });
    }
    let mut content = Vec::new();
    for root in &tree.roots {
        walk(&root.children, &mut content);
    }
    Ok(Content::new(content))
}

fn names_no_scrollable() -> AgentError {
    AgentError::InvalidArgument {
        reason: "a point names no scrollable",
    }
}

impl Serve for method::ElementScrollUntil {
    fn record(outcome: &ActOutcome, issued: &mut IssuedHandles) {
        record_issued_outcome(outcome, issued);
    }

    fn serve(
        ctx: &mut ServeContext<'_, ActOutcome>,
        params: ScrollUntilParams,
    ) -> Answer<ActOutcome> {
        if matches!(params.target, Target::Point { .. }) {
            return Answer::Now(Err(names_no_scrollable()));
        }
        if let Err(error) = takes_semantic_path(&params.options, Forms::Synthetic) {
            return Answer::Now(Err(error));
        }
        let Some(lease) = ctx.lease else {
            return Answer::Now(Err(unleased()));
        };
        let scrollable = match Aimed::try_new(&params.target, ctx.window) {
            Ok(scrollable) => scrollable,
            Err(error) => return Answer::Now(Err(error)),
        };
        let snapshot = params.options.snapshot;
        let before = match agent::tree_before(ctx.state, ctx.window, snapshot) {
            Ok(before) => before,
            Err(error) => return Answer::Now(Err(error)),
        };
        let search = ScrollSearch {
            scrollable,
            until: params.until,
            direction: params.direction.unwrap_or(Direction::Down),
            may_turn: params.direction.is_none(),
            moved: false,
            snapshot,
            before,
            stepping: None,
            sent_over: None,
            observed: None,
        };
        let deadline = deadline_of(ctx.state.clock().now(), &params.options);
        let held = HeldAct::new(deadline, Some(lease), Holding::Scroll(search), ctx.park());
        ctx.runtime
            .hold_act(ctx.state, held, &ctx.if_panics, &mut ctx.caught);
        Answer::Later
    }
}
