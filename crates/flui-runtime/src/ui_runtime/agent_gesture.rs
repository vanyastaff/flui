//! The gestures the agent answers in the application (ADR-0400 §2): `element.tap`, `double_tap`,
//! `long_press`, `drag_to`, `swipe`, `fling`, `pinch`, `type_text`, `press_keys` and
//! `scroll_into_view` (`scroll_until` is [`agent_scroll`](super::agent_scroll)'s).
//!
//! Each is held ([`agent_held`](super::agent_held)) until its target is ready, then performed in
//! the turn it passed, aimed at that frame's bounds:
//!
//! - **Ready.** A locator is resolved again at each attempt, in the application: none holds on
//!   (and answers `not_found` at the timeout), several under a strict pick answer `ambiguous`
//!   with every candidate. The element must pass every actionability check on one frame
//!   (attached, visible, stable over two frames, enabled, receives the hit; the check
//!   `element.actionability` answers with), or only attached under `force`. An element handle
//!   that is no longer attached answers `gone`. A point the call names is ready at once.
//! - **Path.** The call's `via`, synthetic when it names none. Synthetic input is composed by
//!   [`gestures`](flui_protocol::input::gestures) from where the act lands and injected through
//!   the call's input lease, as `input.inject` is. The semantic path performs the semantics action
//!   that does the same where the gesture has one (a primary tap invokes, a focus before typing
//!   focuses), and otherwise falls back to synthetic input unless the call requires the semantic
//!   path. The `os` path is the driver's: the agent refuses it.
//! - **Answer.** What performing it answers: the injection's outcome once a frame has passed since
//!   its last tick, or the semantics action's, naming the element acted on.

use std::time::Duration;

use flui_protocol::input::gestures::{self, Aim, GestureError};
use flui_protocol::method::{self, Method};
use flui_protocol::{
    act::ActOutcome, act::ActionRequest, act::ActionabilityCheck, act::ActionabilityParams,
    geometry::LogicalPoint, input::Direction, input::DragParams, input::GestureOptions,
    input::InputPath, input::LongPressParams, input::PinchParams, input::PointerButton,
    input::PressKeysParams, input::ScrollIntoViewParams, input::SwipeParams, input::TapParams,
    input::TypeTextParams, locator::Locator, locator::Target, snapshot::Snapshot, tree::ElementRef,
    tree::WindowId, wire::ActionName,
};

use super::agent::{
    self, AgentError, IssuedHandles, ReplySender, Served, record_issued_outcome, send_reply,
};
use super::agent_held::{Attempt, AttemptContext, HeldAct, Holding};
use super::agent_input::{self, Injecting, Script};
use super::agent_serve::{Answer, Serve, ServeContext};

/// What a gesture aims at, as the call named it.
pub(super) enum Aimed {
    /// An element, by its handle.
    Element(ElementRef),
    /// The element a locator resolves to at each attempt.
    Locator(Locator),
    /// A point in the window.
    Point(LogicalPoint),
}

impl Aimed {
    /// `target`, asked of `window`.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a point in another window, or a target of a kind this agent does
    /// not know.
    pub(super) fn of(target: &Target, window: WindowId) -> Result<Self, AgentError> {
        match target {
            Target::Element(element) => Ok(Self::Element(*element)),
            Target::Locator(locator) => Ok(Self::Locator(locator.clone())),
            Target::Point { window: named, at } if *named == window => Ok(Self::Point(*at)),
            Target::Point { .. } => Err(AgentError::InvalidArgument {
                reason: "a gesture's points are in the window it is asked of",
            }),
            _ => Err(AgentError::InvalidArgument {
                reason: "this app's agent does not know that kind of target",
            }),
        }
    }

    const fn is_point(&self) -> bool {
        matches!(self, Self::Point(_))
    }
}

/// What a target must pass before the act performs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Checks {
    /// Every actionability check.
    All,
    /// Only attached: `force`, or an act that needs no hit (scrolling an element into view).
    Attached,
}

/// Where a target stands at one attempt.
pub(super) enum Readiness {
    /// Ready: the element (none for a point) and where the act lands (none for an element with
    /// no visible part, which only an act that needs no point can take).
    Ready(Option<ElementRef>, Option<Aim>),
    /// Not yet: why, which a timeout answers.
    NotYet(AgentError),
    /// Never: the answer.
    Failed(AgentError),
}

/// Where `aimed` stands in `ctx`'s window now, against `checks`.
pub(super) fn readiness(ctx: &mut AttemptContext<'_>, aimed: &Aimed, checks: Checks) -> Readiness {
    let (element, located) = match aimed {
        Aimed::Point(at) => return Readiness::Ready(None, Some(Aim::new(*at, None))),
        Aimed::Element(element) => (*element, false),
        Aimed::Locator(locator) => match agent::resolve(ctx.state, ctx.window, locator) {
            Ok(element) => (ElementRef::new(ctx.window, element), true),
            Err(error @ (AgentError::NoMatch | AgentError::NoTreeYet)) => {
                return Readiness::NotYet(error);
            }
            Err(error) => return Readiness::Failed(error),
        },
    };
    ctx.if_panics.set(AgentError::ResolvePanicked {
        element: element.element,
    });
    let facts = match agent::actionability(
        ctx.state,
        ctx.window,
        &ActionabilityParams::new(element),
        ctx.caught,
    ) {
        Ok(facts) => facts,
        Err(
            error @ (AgentError::NoTreeYet | AgentError::RenderTreeBusy | AgentError::TreeBusy(_)),
        ) => return Readiness::NotYet(error),
        Err(error) => return Readiness::Failed(error),
    };
    if !facts.attached {
        // A locator resolves again at the next attempt; a handle names an element that is gone.
        return if located {
            Readiness::NotYet(AgentError::NoMatch)
        } else {
            Readiness::Failed(AgentError::NodeNotFound {
                element: element.element,
                issued: agent::was_issued(ctx.state, element.element),
            })
        };
    }
    let unmet = match checks {
        Checks::All => facts.first_failed(),
        Checks::Attached => None,
    };
    if let Some(failed) = unmet {
        return Readiness::NotYet(AgentError::NotActionable {
            failed,
            covered_by: facts.covered_by,
        });
    }
    let aim = facts
        .point
        .or_else(|| facts.rect.map(|rect| rect.center()))
        .map(|point| Aim::new(point, facts.rect));
    Readiness::Ready(Some(element), aim)
}

/// Why an act that needs a point has none: its element shows no visible part.
fn no_point() -> AgentError {
    AgentError::NotActionable {
        failed: ActionabilityCheck::Visible,
        covered_by: None,
    }
}

/// The pointer input a gesture composes from where it lands.
pub(super) enum Composed {
    Tap(PointerButton),
    DoubleTap(PointerButton),
    LongPress(Option<u64>),
    Drag,
    Swipe {
        direction: Direction,
        distance: Option<f64>,
        velocity: Option<f64>,
        fling: bool,
    },
    Pinch(f64),
}

impl Composed {
    /// The sequence, landing at `aim` and, for a drag, dropped at `to`.
    fn sequence(
        &self,
        aim: Aim,
        to: Option<Aim>,
    ) -> Result<flui_protocol::input::ActionSequence, GestureError> {
        Ok(match *self {
            Self::Tap(button) => gestures::tap(aim.point, button),
            Self::DoubleTap(button) => gestures::double_tap(aim.point, button),
            Self::LongPress(duration_ms) => gestures::long_press(aim.point, duration_ms),
            Self::Drag => gestures::drag(aim.point, to.map_or(aim.point, |to| to.point)),
            Self::Swipe {
                direction,
                distance,
                velocity,
                fling,
            } => gestures::swipe(aim, direction, distance, velocity, fling)?,
            Self::Pinch(scale) => gestures::pinch(aim, scale)?,
        })
    }
}

/// What a gesture does once its target is ready.
pub(super) enum Perform {
    /// Pointer input composed from where it lands.
    Input(Composed),
    /// Focus the target, then this input: the focusing tap, or the semantics focus on the
    /// semantic path.
    FocusThen(Option<Script>),
    /// A semantics action on the element.
    Semantic(ActionName),
}

/// A gesture held until its targets are ready.
pub(super) struct GestureAct {
    target: Aimed,
    /// A drag's drop target.
    to: Option<Aimed>,
    checks: Checks,
    perform: Perform,
    /// On the semantic path: a focus is the semantics action.
    semantic: bool,
    snapshot: Snapshot,
}

impl GestureAct {
    /// One attempt: perform when every target is ready, hold on otherwise, and answer the reason
    /// at the last attempt.
    pub(super) fn attempt(
        &mut self,
        ctx: &mut AttemptContext<'_>,
        reply: &ReplySender<ActOutcome>,
    ) -> Attempt {
        let (element, aim) = match readiness(ctx, &self.target, self.checks) {
            Readiness::Ready(element, aim) => (element, aim),
            Readiness::NotYet(why) => return not_yet(ctx, reply, why),
            Readiness::Failed(error) => return answer(reply, Err(error)),
        };
        let to = match &self.to {
            None => None,
            Some(to) => match readiness(ctx, to, self.checks) {
                Readiness::Ready(_, Some(to)) => Some(to),
                Readiness::Ready(_, None) => return not_yet(ctx, reply, no_point()),
                Readiness::NotYet(why) => return not_yet(ctx, reply, why),
                Readiness::Failed(error) => return answer(reply, Err(error)),
            },
        };
        let snapshot = self.snapshot;
        let semantic = self.semantic;
        match &mut self.perform {
            Perform::Semantic(action) => {
                let Some(element) = element else {
                    return answer(reply, Err(names_no_element()));
                };
                semantic_act(ctx, reply, element, *action, snapshot)
            }
            Perform::Input(composed) => {
                // Pointer input needs a point to land on.
                let Some(aim) = aim else {
                    return not_yet(ctx, reply, no_point());
                };
                match composed
                    .sequence(aim, to)
                    .map_err(|error| AgentError::UnplayableInput(error.to_string()))
                    .and_then(|sequence| agent_input::lower(&sequence))
                {
                    Ok(script) => inject(ctx, reply, element, script, snapshot),
                    Err(error) => answer(reply, Err(error)),
                }
            }
            Perform::FocusThen(then) if semantic => {
                let Some(element) = element else {
                    return answer(reply, Err(names_no_element()));
                };
                if let Err(error) = focus(ctx, element) {
                    return answer(reply, Err(error));
                }
                let then = then.take().expect("BUG: a gesture performs once");
                inject(ctx, reply, Some(element), then, snapshot)
            }
            Perform::FocusThen(then) => {
                let Some(aim) = aim else {
                    return not_yet(ctx, reply, no_point());
                };
                let then = then.take().expect("BUG: a gesture performs once");
                match agent_input::lower(&gestures::tap(aim.point, PointerButton::Primary)) {
                    Ok(tap) => inject(ctx, reply, element, tap.then(then), snapshot),
                    Err(error) => answer(reply, Err(error)),
                }
            }
        }
    }
}

/// The targets are not ready for `why`: hold on, or answer it at the last attempt, which runs on
/// the frame the deadline passed on.
fn not_yet(ctx: &AttemptContext<'_>, reply: &ReplySender<ActOutcome>, why: AgentError) -> Attempt {
    if ctx.last {
        return answer(reply, Err(why));
    }
    Attempt::Hold
}

/// Answer `reply` with `result`: the act is done.
fn answer(reply: &ReplySender<ActOutcome>, result: Result<ActOutcome, AgentError>) -> Attempt {
    send_reply(reply, None, result);
    Attempt::Done
}

fn names_no_element() -> AgentError {
    AgentError::NotSupported {
        capability: method::InputInject::NAME,
        reason: "a semantics action takes an element; a point takes input",
    }
}

/// Queue `script` in `ctx`'s window through the act's lease; the injection answers `reply`.
fn inject(
    ctx: &AttemptContext<'_>,
    reply: &ReplySender<ActOutcome>,
    element: Option<ElementRef>,
    script: Script,
    snapshot: Snapshot,
) -> Attempt {
    let Some(lease) = ctx.lease else {
        return answer(reply, Err(unleased()));
    };
    let before = match agent::tree_before(ctx.state, ctx.window, snapshot) {
        Ok(before) => before,
        Err(error) => return answer(reply, Err(error)),
    };
    let injecting = Injecting {
        lease,
        script,
        element,
        snapshot,
        before,
    };
    ctx.runtime
        .queue_injection(ctx.state, injecting, reply.clone());
    Attempt::Done
}

/// Run `action` on `element` through the path an assistive technology's action takes, and
/// answer `reply` with what it ran, once the reading it owes is taken.
fn semantic_act(
    ctx: &AttemptContext<'_>,
    reply: &ReplySender<ActOutcome>,
    element: ElementRef,
    action: ActionName,
    snapshot: Snapshot,
) -> Attempt {
    ctx.if_panics.set(AgentError::ResolvePanicked {
        element: element.element,
    });
    let request = ActionRequest::new(element.element, action);
    match agent::act(
        ctx.state,
        ctx.window,
        &request,
        snapshot,
        true,
        ctx.if_panics,
    ) {
        Ok(Served::Answered(outcome)) => answer(reply, Ok(outcome)),
        Ok(Served::Reading(owed)) => {
            // The reading is owed once the effect settled; the reply waits with it.
            ctx.runtime.park_act_read(ctx.state, owed, reply.clone());
            Attempt::Done
        }
        Err(error) => answer(reply, Err(error)),
    }
}

/// Give `element` focus through its semantics action, reading nothing back.
fn focus(ctx: &AttemptContext<'_>, element: ElementRef) -> Result<(), AgentError> {
    ctx.if_panics.set(AgentError::ResolvePanicked {
        element: element.element,
    });
    let request = ActionRequest::new(element.element, ActionName::Focus);
    agent::act(
        ctx.state,
        ctx.window,
        &request,
        Snapshot::None,
        true,
        ctx.if_panics,
    )
    .map(|_| ())
}

fn unleased() -> AgentError {
    AgentError::InvalidArgument {
        reason: "a gesture's synthetic input is asked through an input lease",
    }
}

/// Whether a gesture takes the semantic path under `options`, where `semantic_form` says it has
/// a semantics action that does the same: the path the call names, synthetic when it names none,
/// and synthetic in place of a semantic path the gesture has no action for unless the call
/// requires it.
///
/// # Errors
///
/// `not_supported` for the `os` path, which the driver composes with its lane, and for a semantic
/// path the call requires and the gesture has no action for.
pub(super) fn takes_semantic_path(
    options: &GestureOptions,
    semantic_form: bool,
) -> Result<bool, AgentError> {
    let policy = options.via.unwrap_or_default();
    match policy.prefer {
        InputPath::Synthetic => Ok(false),
        InputPath::Semantic if semantic_form => Ok(true),
        InputPath::Semantic if !policy.require => Ok(false),
        InputPath::Semantic => Err(AgentError::NotSupported {
            capability: "semantic_input",
            reason: "this gesture has no semantics action that does the same",
        }),
        _ => Err(AgentError::NotSupported {
            capability: "os_input",
            reason: "the os path is composed by the driver with its lane, not in the app",
        }),
    }
}

/// When an act asked at the presentation's `now` with `options` gives up.
pub(super) fn deadline_of(now: web_time::Instant, options: &GestureOptions) -> web_time::Instant {
    let timeout = Duration::from_millis(options.effective_timeout_ms());
    now.checked_add(timeout).unwrap_or(now)
}

/// A gesture as the call asks it, to be held.
struct Asked<'p> {
    target: &'p Target,
    to: Option<&'p Target>,
    options: &'p GestureOptions,
    perform: Perform,
    /// What the gesture's target must pass, `force` aside.
    checks: Checks,
    semantic: bool,
}

/// Hold the gesture `asked` in `ctx`'s window.
fn hold(ctx: &mut ServeContext<'_, ActOutcome>, asked: Asked<'_>) -> Answer<ActOutcome> {
    let Asked {
        target,
        to,
        options,
        perform,
        checks,
        semantic,
    } = asked;
    let target = match Aimed::of(target, ctx.window) {
        Ok(target) => target,
        Err(error) => return Answer::Now(Err(error)),
    };
    let to = match to.map(|to| Aimed::of(to, ctx.window)).transpose() {
        Ok(to) => to,
        Err(error) => return Answer::Now(Err(error)),
    };
    let injects = match &perform {
        Perform::Semantic(_) => false,
        Perform::Input(_) | Perform::FocusThen(_) => true,
    };
    if injects && ctx.lease.is_none() {
        return Answer::Now(Err(unleased()));
    }
    let checks = if options.force {
        Checks::Attached
    } else {
        checks
    };
    let act = GestureAct {
        target,
        to,
        checks,
        perform,
        semantic,
        snapshot: options.snapshot,
    };
    let deadline = deadline_of(ctx.state.clock().now(), options);
    let lease = if injects { ctx.lease } else { None };
    let held = HeldAct::new(deadline, lease, Holding::Gesture(act), ctx.park());
    ctx.runtime
        .hold_act(ctx.state, held, &ctx.if_panics, &mut ctx.caught);
    Answer::Later
}

/// Hold a gesture whose input is composed from where it lands and that no semantics action does.
fn hold_input(
    ctx: &mut ServeContext<'_, ActOutcome>,
    target: &Target,
    to: Option<&Target>,
    options: &GestureOptions,
    composed: Composed,
) -> Answer<ActOutcome> {
    if let Err(error) = takes_semantic_path(options, false) {
        return Answer::Now(Err(error));
    }
    hold(
        ctx,
        Asked {
            target,
            to,
            options,
            perform: Perform::Input(composed),
            checks: Checks::All,
            semantic: false,
        },
    )
}

/// Hold a gesture that focuses `target`, then plays `then`: committed text or key chords, which
/// only synthetic input carries.
fn hold_focus_then(
    ctx: &mut ServeContext<'_, ActOutcome>,
    target: &Target,
    options: &GestureOptions,
    then: Script,
) -> Answer<ActOutcome> {
    let requires_semantic = options
        .via
        .is_some_and(|via| via.require && via.prefer == InputPath::Semantic);
    if requires_semantic {
        return Answer::Now(Err(AgentError::NotSupported {
            capability: "semantic_input",
            reason: "text and keys have no semantics action that does the same",
        }));
    }
    let semantic = match takes_semantic_path(options, true) {
        Ok(semantic) => semantic && !matches!(target, Target::Point { .. }),
        Err(error) => return Answer::Now(Err(error)),
    };
    hold(
        ctx,
        Asked {
            target,
            to: None,
            options,
            perform: Perform::FocusThen(Some(then)),
            checks: Checks::All,
            semantic,
        },
    )
}

/// A gesture's answer records the element it acted on, as an act's does.
macro_rules! records_its_element {
    () => {
        fn record(outcome: &ActOutcome, issued: &mut IssuedHandles) {
            record_issued_outcome(outcome, issued);
        }
    };
}

impl Serve for method::ElementTap {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: TapParams) -> Answer<ActOutcome> {
        let invoke = gestures::tap_action(params.button);
        let target = match Aimed::of(&params.target, ctx.window) {
            Ok(target) => target,
            Err(error) => return Answer::Now(Err(error)),
        };
        let semantic_form = invoke.is_some() && !target.is_point();
        let semantic = match takes_semantic_path(&params.options, semantic_form) {
            Ok(semantic) => semantic,
            Err(error) => return Answer::Now(Err(error)),
        };
        let perform = match invoke {
            Some(action) if semantic => Perform::Semantic(action),
            _ => Perform::Input(Composed::Tap(params.button)),
        };
        hold(
            ctx,
            Asked {
                target: &params.target,
                to: None,
                options: &params.options,
                perform,
                checks: Checks::All,
                semantic,
            },
        )
    }
}

impl Serve for method::ElementDoubleTap {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: TapParams) -> Answer<ActOutcome> {
        let composed = Composed::DoubleTap(params.button);
        hold_input(ctx, &params.target, None, &params.options, composed)
    }
}

impl Serve for method::ElementLongPress {
    records_its_element!();

    fn serve(
        ctx: &mut ServeContext<'_, ActOutcome>,
        params: LongPressParams,
    ) -> Answer<ActOutcome> {
        let composed = Composed::LongPress(params.duration_ms);
        hold_input(ctx, &params.target, None, &params.options, composed)
    }
}

impl Serve for method::ElementDragTo {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: DragParams) -> Answer<ActOutcome> {
        hold_input(
            ctx,
            &params.target,
            Some(&params.to),
            &params.options,
            Composed::Drag,
        )
    }
}

/// Serve `element.swipe` (`fling` false) or `element.fling`.
fn serve_stroke(
    ctx: &mut ServeContext<'_, ActOutcome>,
    params: &SwipeParams,
    fling: bool,
) -> Answer<ActOutcome> {
    let composed = Composed::Swipe {
        direction: params.direction,
        distance: params.distance,
        velocity: params.velocity,
        fling,
    };
    hold_input(ctx, &params.target, None, &params.options, composed)
}

impl Serve for method::ElementSwipe {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: SwipeParams) -> Answer<ActOutcome> {
        serve_stroke(ctx, &params, false)
    }
}

impl Serve for method::ElementFling {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: SwipeParams) -> Answer<ActOutcome> {
        serve_stroke(ctx, &params, true)
    }
}

impl Serve for method::ElementPinch {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: PinchParams) -> Answer<ActOutcome> {
        let composed = Composed::Pinch(params.scale);
        hold_input(ctx, &params.target, None, &params.options, composed)
    }
}

impl Serve for method::ElementScrollIntoView {
    records_its_element!();

    /// The semantics action that scrolls each scrollable ancestor, once the target is attached:
    /// it need not be actionable, since it may be out of view.
    fn serve(
        ctx: &mut ServeContext<'_, ActOutcome>,
        params: ScrollIntoViewParams,
    ) -> Answer<ActOutcome> {
        if matches!(params.target, Target::Point { .. }) {
            return Answer::Now(Err(AgentError::InvalidArgument {
                reason: "a point names no element to scroll into view",
            }));
        }
        hold(
            ctx,
            Asked {
                target: &params.target,
                to: None,
                options: &params.options,
                perform: Perform::Semantic(ActionName::ScrollIntoView),
                checks: Checks::Attached,
                semantic: true,
            },
        )
    }
}

impl Serve for method::ElementTypeText {
    records_its_element!();

    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: TypeTextParams) -> Answer<ActOutcome> {
        let text = match agent_input::lower(&gestures::text(&params.text)) {
            Ok(text) => text,
            Err(error) => return Answer::Now(Err(error)),
        };
        hold_focus_then(ctx, &params.target, &params.options, text)
    }
}

impl Serve for method::ElementPressKeys {
    records_its_element!();

    /// Focuses the target when there is one, then presses each chord; with none, the keys go to
    /// whatever holds the window's focus, at once.
    fn serve(
        ctx: &mut ServeContext<'_, ActOutcome>,
        params: PressKeysParams,
    ) -> Answer<ActOutcome> {
        let keys = gestures::chords(&params.keys)
            .map_err(|error| AgentError::UnplayableInput(error.to_string()))
            .and_then(|sequence| agent_input::lower(&sequence));
        let keys = match keys {
            Ok(keys) => keys,
            Err(error) => return Answer::Now(Err(error)),
        };
        if let Some(target) = &params.target {
            return hold_focus_then(ctx, target, &params.options, keys);
        }
        if let Err(error) = takes_semantic_path(&params.options, false) {
            return Answer::Now(Err(error));
        }
        let Some(lease) = ctx.lease else {
            return Answer::Now(Err(unleased()));
        };
        let before = match agent::tree_before(ctx.state, ctx.window, params.options.snapshot) {
            Ok(before) => before,
            Err(error) => return Answer::Now(Err(error)),
        };
        let injecting = Injecting {
            lease,
            script: keys,
            element: None,
            snapshot: params.options.snapshot,
            before,
        };
        ctx.runtime
            .queue_injection(ctx.state, injecting, ctx.park());
        Answer::Later
    }
}
