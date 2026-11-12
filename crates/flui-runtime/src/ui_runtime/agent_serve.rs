//! How the owner answers the protocol's window methods (ADR-0238 §1): one [`Serve`] impl per
//! method, and one place, [`UiRuntime::serve_window_call`], that runs any of them.
//!
//! Every call reaches the owner as one [`WindowCall`] in one command. What every method needs
//! around its own work is written once here: the presentation looked up, a panic contained and
//! answered before it continues, the request counted as invoked or stale, the answer's element
//! handles recorded, and an error turned into the protocol's error object with the handle it is
//! about. The dispatch is generated from the protocol's table
//! ([`flui_protocol::for_each_window_method!`]), so a window method the table gains does not
//! compile here until it has an impl: one that answers it, or one in `not_answered!` that
//! refuses it `not_supported`.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_protocol::method::{self, Method, WindowMethod};
use flui_protocol::{
    ActOutcome, ActParams, ActionRequest, ActionabilityParams, AnnouncementJournal,
    AnnouncementsParams, InputParams, InputPath, InspectParams, InspectReading, Journal,
    JournalParams, LocateParams, Locator, Matches, MotionParams, MotionState, Pick, ReadParams,
    Released, Target, Tree, WaitOutcome, WaitParams, WindowId, WindowParams,
};
use flui_view::__runtime::{LeaseId, Reply, WindowCall};

use super::UiRuntime;
use super::agent::{
    self, AgentError, IssuedHandles, ReplySender, Served, record_issued, record_issued_matches,
    record_issued_outcome, record_issued_wait, send_reply,
};
use super::agent_input;
use super::agent_inspect::InspectTarget;
use super::agent_journal;
use super::commands::DrainReport;
use crate::presentation::PresentationState;

/// How many matches a strict locator is resolved over: two say it is ambiguous, ten name the
/// candidates.
const STRICT_CANDIDATES: u32 = 10;

/// What serving a request came to.
pub(super) enum Answer<T> {
    /// The answer, sent at this drain.
    Now(Result<T, AgentError>),
    /// The handler kept the reply ([`ServeCx::park`]) and answers later.
    Later,
}

/// What a panic while serving answers, as far as serving has got. A step that runs app code
/// sets it before it runs.
pub(super) struct PanicAnswer(RefCell<AgentError>);

impl PanicAnswer {
    /// A panic from here on answers `failure`.
    pub(super) fn set(&self, failure: AgentError) {
        *self.0.borrow_mut() = failure;
    }

    fn take(&self) -> AgentError {
        self.0.replace(AgentError::Panicked)
    }
}

/// What a handler serves one request with. Owner thread only.
pub(super) struct ServeCx<'a, T> {
    pub(super) runtime: &'a UiRuntime,
    pub(super) state: &'a PresentationState,
    pub(super) window: WindowId,
    reply: ReplySender<T>,
    /// The presentation's record of the element handles its answers reported.
    issued: Rc<RefCell<IssuedHandles>>,
    /// What a panic answers.
    pub(super) if_panics: PanicAnswer,
    /// A panic a step caught and handed back instead of unwinding (the hit-test's): the first
    /// failure, resumed once the request is answered.
    pub(super) caught: FirstPanic,
    /// The input lease the call was asked through, if any.
    pub(super) lease: Option<LeaseId>,
    /// The owner's frame is woken once the answer is sent.
    wake_after: bool,
}

impl<T> ServeCx<'_, T> {
    /// The reply, for a handler that answers later. The context keeps a clone, so a panic
    /// after this still answers; the first answer sent is the one taken.
    pub(super) fn park(&self) -> ReplySender<T> {
        self.reply.clone()
    }

    /// Whether an answer of this presentation reported `element`.
    pub(super) fn was_issued(&self, element: flui_protocol::ElementId) -> bool {
        self.issued.borrow().was_issued(element)
    }

    /// Wake the owner's frame once the answer is sent: the wake reaches the platform, user code,
    /// which must not stand between the request and its answer.
    pub(super) fn wake_frame_after(&mut self) {
        self.wake_after = true;
    }
}

/// How the owner answers a window method.
pub(super) trait Serve: WindowMethod {
    /// Whether this build's windows answer it. A method in `not_answered!` does not.
    const ANSWERS: bool = true;

    /// Records the element handles an answer reports.
    fn record(_output: &Self::Output, _issued: &mut IssuedHandles) {}

    /// Serve `params` in `ctx`'s presentation.
    fn serve(ctx: &mut ServeCx<'_, Self::Output>, params: Self::Params) -> Answer<Self::Output>;
}

impl UiRuntime {
    /// Runs `M` on `params` in `presentation` and answers `reply`: see the module docs.
    fn run_window_method<M: Serve>(
        &self,
        presentation: PresentationId,
        params: M::Params,
        lease: Option<LeaseId>,
        reply: Reply<M::Output>,
        report: &mut DrainReport,
    ) {
        let window = super::agent::window_of(presentation);
        let Some(state) = self.presentations.get(presentation) else {
            // Accepted earlier, but the presentation closed first.
            reply.send(Err(AgentError::PresentationGone.into_error(window)));
            report.dropped_stale += 1;
            return;
        };
        let issued = super::agent::issued_of(state);
        let mut ctx = ServeCx {
            runtime: self,
            state,
            window,
            reply: ReplySender::new(reply, window, Rc::clone(&issued), M::record),
            issued,
            if_panics: PanicAnswer(RefCell::new(AgentError::Panicked)),
            caught: FirstPanic::new(),
            lease,
            wake_after: false,
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a method's serve runs contained so its caller is answered"
        )]
        let served = catch_unwind(AssertUnwindSafe(|| M::serve(&mut ctx, params)));
        let ServeCx {
            reply,
            if_panics,
            caught,
            wake_after,
            ..
        } = ctx;
        match (served, caught.into_payload()) {
            (Ok(answer), None) => {
                let stale = matches!(answer, Answer::Now(Err(AgentError::PresentationGone)));
                if stale {
                    report.dropped_stale += 1;
                } else {
                    report.invoked += 1;
                }
                if let Answer::Now(result) = answer {
                    send_reply(&reply, None, result);
                }
                if wake_after {
                    self.wake_frame();
                }
            }
            // A step caught a panic and answered with its own failure: that answer goes, and the
            // caught panic continues.
            (Ok(answer), Some(payload)) => {
                let result = match answer {
                    Answer::Now(result) => result,
                    Answer::Later => Err(if_panics.take()),
                };
                self.fail_agent_job(&reply, None, result, payload);
            }
            // A caught panic is the first failure; one after it is traced and not resumed.
            (Err(second), Some(payload)) => {
                let mut first = FirstPanic::new();
                first.record(payload);
                first.record(second);
                let payload = first.into_payload().expect("BUG: the caught panic is kept");
                self.fail_agent_job(&reply, None, Err(if_panics.take()), payload);
            }
            (Err(payload), None) => {
                self.fail_agent_job(&reply, None, Err(if_panics.take()), payload);
            }
        }
    }

    /// Answers `call` at the owner's drain: the window method it is, run in `presentation`.
    pub(super) fn serve_window_call(
        &self,
        presentation: PresentationId,
        call: WindowCall,
        report: &mut DrainReport,
    ) {
        macro_rules! dispatch {
            ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {{
                let lease = call.lease();
                $(
                    let call = match call.open::<method::$ty>() {
                        Ok((params, reply)) => {
                            return self.run_window_method::<method::$ty>(
                                presentation, params, lease, reply, report,
                            );
                        }
                        Err(call) => call,
                    };
                )+
                unreachable!("BUG: a window call is one of the table's window methods: {call:?}");
            }};
        }
        flui_protocol::for_each_window_method!(dispatch)
    }
}

/// Whether this build's windows answer the window method named `name`: a method of the table
/// with an impl that answers it.
#[must_use]
pub fn window_answers(name: &str) -> bool {
    macro_rules! answers {
        ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {
            match name {
                $($name => <method::$ty as Serve>::ANSWERS,)+
                _ => false,
            }
        };
    }
    flui_protocol::for_each_window_method!(answers)
}

/// Window methods this build's windows do not answer yet: each answers `not_supported`, naming
/// itself. The list only shrinks, as each gains an impl that answers it.
macro_rules! not_answered {
    ($($ty:ident),+ $(,)?) => {$(
        impl Serve for method::$ty {
            const ANSWERS: bool = false;

            fn serve(_: &mut ServeCx<'_, Self::Output>, _: Self::Params) -> Answer<Self::Output> {
                Answer::Now(Err(AgentError::NotSupported {
                    capability: Self::NAME,
                    reason: "this app's windows do not answer it yet",
                }))
            }
        }
    )+};
}

not_answered!(
    AuditA11y,
    CapturePresented,
    CaptureRendered,
    PerfStart,
    PerfStop,
    WaitFrames,
    WaitSettled,
    WindowPlacement,
);

impl Serve for method::SemanticsRead {
    fn record(tree: &Tree, issued: &mut IssuedHandles) {
        record_issued(tree, issued);
    }

    fn serve(ctx: &mut ServeCx<'_, Tree>, params: ReadParams) -> Answer<Tree> {
        let query = params.query;
        let issued = query.root.is_none_or(|root| ctx.was_issued(root));
        Answer::Now(agent::read_tree(ctx.state, ctx.window, &query, issued))
    }
}

impl Serve for method::InputInject {
    fn record(outcome: &ActOutcome, issued: &mut IssuedHandles) {
        record_issued_outcome(outcome, issued);
    }

    /// Plays the actions through the call's lease, one tick per frame boundary, and answers once
    /// a frame has passed since the last one, with the reading the call asks for.
    fn serve(ctx: &mut ServeCx<'_, ActOutcome>, params: InputParams) -> Answer<ActOutcome> {
        let Some(lease) = ctx.lease else {
            return Answer::Now(Err(AgentError::InvalidArgument {
                reason: "input.inject is asked through an input lease",
            }));
        };
        if params
            .via
            .is_some_and(|via| via.require && via.prefer != InputPath::Synthetic)
        {
            return Answer::Now(Err(AgentError::InvalidArgument {
                reason: "input.inject is synthetic input; another path is input.perform's",
            }));
        }
        let plan = match agent_input::plan(&params.actions) {
            Ok(plan) => plan,
            Err(error) => return Answer::Now(Err(error)),
        };
        let before = match agent::tree_before(ctx.state, ctx.window, params.snapshot) {
            Ok(before) => before,
            Err(error) => return Answer::Now(Err(error)),
        };
        ctx.runtime
            .queue_injection(ctx.state, lease, plan, params.snapshot, before, ctx.park());
        Answer::Later
    }
}

impl Serve for method::InputRelease {
    /// Lets go what the call's lease holds once the work queued before it has run.
    fn serve(ctx: &mut ServeCx<'_, Released>, _: WindowParams) -> Answer<Released> {
        let Some(lease) = ctx.lease else {
            return Answer::Now(Err(AgentError::InvalidArgument {
                reason: "input.release is asked through an input lease",
            }));
        };
        ctx.runtime.queue_release(ctx.state, lease, ctx.park());
        Answer::Later
    }
}

impl Serve for method::SemanticsLocate {
    fn record(matches: &Matches, issued: &mut IssuedHandles) {
        record_issued_matches(matches, issued);
    }

    fn serve(ctx: &mut ServeCx<'_, Matches>, params: LocateParams) -> Answer<Matches> {
        Answer::Now(agent::locate(ctx.state, ctx.window, &params))
    }
}

impl Serve for method::SemanticsAct {
    fn record(outcome: &ActOutcome, issued: &mut IssuedHandles) {
        record_issued_outcome(outcome, issued);
    }

    /// The target is resolved and the action run in this one turn, so the element acted on is
    /// the one the locator matched on the tree the action starts on.
    fn serve(ctx: &mut ServeCx<'_, ActOutcome>, params: ActParams) -> Answer<ActOutcome> {
        let (element, issued) = match params.target {
            Target::Element(element) => (element.element, ctx.was_issued(element.element)),
            // The element comes from the tree the action runs on.
            Target::Locator(locator) => match resolve(ctx, &locator) {
                Ok(element) => (element, true),
                Err(error) => return Answer::Now(Err(error)),
            },
            Target::Point { .. } => {
                return Answer::Now(Err(AgentError::NotSupported {
                    capability: method::InputInject::NAME,
                    reason: "a semantics action takes an element or a locator; a point takes input",
                }));
            }
            _ => {
                return Answer::Now(Err(AgentError::InvalidArgument {
                    reason: "this app's agent does not know that kind of target",
                }));
            }
        };
        let mut request = ActionRequest::new(element, params.action);
        request.value = params.value;
        ctx.if_panics.set(AgentError::ResolvePanicked { element });
        let served = agent::act(
            ctx.state,
            ctx.window,
            &request,
            params.snapshot,
            issued,
            &ctx.if_panics,
        );
        match served {
            Ok(Served::Answered(outcome)) => Answer::Now(Ok(outcome)),
            Ok(Served::Reading(owed)) => {
                // The reading is owed once the effect settled; the reply waits with it.
                ctx.runtime.park_act_read(ctx.state, owed, ctx.park());
                Answer::Later
            }
            Err(error) => Answer::Now(Err(error)),
        }
    }
}

/// The one element `locator` matches in `ctx`'s tree now, as its pick says.
fn resolve<T>(
    ctx: &ServeCx<'_, T>,
    locator: &Locator,
) -> Result<flui_protocol::ElementId, AgentError> {
    let strict = locator.pick == Pick::Strict;
    let mut params = LocateParams::new(locator.clone());
    if strict {
        params.max_matches = Some(STRICT_CANDIDATES);
    }
    let matches = agent::locate(ctx.state, ctx.window, &params)?;
    match matches.matches.as_slice() {
        [] => Err(AgentError::NoMatch),
        [only] => Ok(only.element.element),
        several if strict => Err(AgentError::Ambiguous {
            matched: several.len(),
            candidates: several
                .iter()
                .map(|candidate| {
                    flui_protocol::tree::outline(std::slice::from_ref(&candidate.node))
                })
                .collect(),
        }),
        [first, ..] => Ok(first.element.element),
    }
}

impl Serve for method::ElementActionability {
    fn serve(
        ctx: &mut ServeCx<'_, flui_protocol::Actionability>,
        params: ActionabilityParams,
    ) -> Answer<flui_protocol::Actionability> {
        let element = params.element.element;
        ctx.if_panics.set(AgentError::ResolvePanicked { element });
        Answer::Now(agent::actionability(
            ctx.state,
            ctx.window,
            &params,
            &mut ctx.caught,
        ))
    }
}

impl Serve for method::WaitUntil {
    fn record(outcome: &WaitOutcome, issued: &mut IssuedHandles) {
        record_issued_wait(outcome, issued);
    }

    /// It answers itself: now if the condition holds, else after a frame or at its deadline.
    fn serve(ctx: &mut ServeCx<'_, WaitOutcome>, params: WaitParams) -> Answer<WaitOutcome> {
        ctx.if_panics.set(AgentError::Malformed);
        ctx.runtime.park_agent_wait(ctx.state, params, ctx.park());
        Answer::Later
    }
}

/// Serves a read of the element or render tree at the owner's turn.
fn inspect_now(
    ctx: &ServeCx<'_, InspectReading>,
    target: InspectTarget,
    params: &InspectParams,
) -> Answer<InspectReading> {
    // A render object's `debug_name` or an `inspect` override is app code.
    ctx.if_panics.set(AgentError::InspectPanicked);
    Answer::Now(super::agent_inspect::inspect(ctx.state, target, params))
}

impl Serve for method::InspectWidgets {
    fn serve(
        ctx: &mut ServeCx<'_, InspectReading>,
        params: InspectParams,
    ) -> Answer<InspectReading> {
        inspect_now(ctx, InspectTarget::Widgets, &params)
    }
}

impl Serve for method::InspectRender {
    fn serve(
        ctx: &mut ServeCx<'_, InspectReading>,
        params: InspectParams,
    ) -> Answer<InspectReading> {
        inspect_now(ctx, InspectTarget::Render, &params)
    }
}

impl Serve for method::InspectLayers {
    /// Parked for the next scene the window shows.
    fn serve(
        ctx: &mut ServeCx<'_, InspectReading>,
        params: InspectParams,
    ) -> Answer<InspectReading> {
        ctx.runtime
            .park_agent_layer_read(ctx.state, &params, ctx.park());
        Answer::Later
    }
}

impl Serve for method::MotionSet {
    fn serve(ctx: &mut ServeCx<'_, MotionState>, params: MotionParams) -> Answer<MotionState> {
        let applied = ctx.state.apply_motion(params.request);
        let result = match applied {
            Ok((state, demand)) => {
                if demand {
                    // An accepted inspection sample outlives animation pause or run completion.
                    ctx.state
                        .clock()
                        .mark_demand(flui_scheduler::DemandKind::Host);
                    ctx.wake_frame_after();
                }
                Ok(state)
            }
            Err(_) => Err(AgentError::InvalidArgument {
                reason: "motion rate must be finite and non-negative",
            }),
        };
        Answer::Now(result)
    }
}

impl Serve for method::SemanticsAnnouncements {
    fn serve(
        ctx: &mut ServeCx<'_, AnnouncementJournal>,
        params: AnnouncementsParams,
    ) -> Answer<AnnouncementJournal> {
        Answer::Now(Ok(ctx.state.pipeline().with(|pipeline| {
            pipeline
                .semantics_owner()
                .map(|owner| owner.announcement_journal(&params))
                .unwrap_or_default()
        })))
    }
}

impl Serve for method::InputJournal {
    fn serve(ctx: &mut ServeCx<'_, Journal>, params: JournalParams) -> Answer<Journal> {
        Answer::Now(Ok(agent_journal::read(ctx.state, &params)))
    }
}
