//! How the owner answers the protocol's window methods (ADR-0238 §1): one [`Serve`] impl per
//! method, or one [`Annotate`] impl per method that only annotates, and one place,
//! [`UiRuntime::serve_window_call`], that runs any of them.
//!
//! Every call reaches the owner as one [`WindowCall`] in one command. What every method needs
//! around its own work is written once here: the presentation looked up, a panic contained and
//! answered before it continues, the request counted as invoked or stale, the answer's element
//! handles recorded, and an error turned into the protocol's error object with the handle it is
//! about. The dispatch is generated from the protocol's table
//! ([`flui_protocol::for_each_window_method!`]), so a window method the table gains does not
//! compile here until it has an impl: one that answers it, or one in `not_answered!` that
//! refuses it `not_supported`.
//!
//! # Annotations
//!
//! A method whose effect is [`MethodEffect::Annotates`] changes only what the development tools
//! draw over the window, and an observer may call it (ADR-0395), so it must reach nothing
//! else. Its row is dispatched to [`Annotate`], whose handler is given the presentation's
//! [`BuildBanner`] and its parameters, never a [`ServeContext`], the runtime or the
//! presentation; and `run_window_method` does not build for an annotating method (an assertion
//! evaluated when it is instantiated, so `cargo build` reports it and `cargo check` does not), so
//! a [`Serve`] impl of one is never run. The answer is sent before the frame the change owes is
//! requested, under the same containment and stale-presentation rules as every other method.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_protocol::method::{self, Method, MethodEffect, WindowMethod};
use flui_protocol::{
    act::ActOutcome, act::ActParams, act::ActionRequest, act::ActionabilityParams,
    build::BuildReportParams, events::AnnouncementJournal, events::AnnouncementsParams,
    geometry::LogicalInsets, geometry::LogicalSize, input::InputParams, input::InputPath,
    input::Journal, input::JournalParams, input::Released, inspect::InspectParams,
    inspect::InspectReading, locator::LocateParams, locator::Locator, locator::Matches,
    locator::Target, motion::MotionParams, motion::MotionState, session::Empty, session::Placement,
    session::Platform, session::WindowParams, tree::ReadParams, tree::Tree, tree::WindowId,
    wait::Condition, wait::FramesParams, wait::SettledParams, wait::WaitOutcome, wait::WaitParams,
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
use crate::build_overlay::{BuildBanner, UnknownBuildState};
use crate::presentation::PresentationState;

/// What serving a request came to.
pub(super) enum Answer<T> {
    /// The answer, sent at this drain.
    Now(Result<T, AgentError>),
    /// The handler kept the reply ([`ServeContext::park`]) and answers later.
    Later,
}

/// What a panic while serving answers, as far as serving has got. A step that runs app code
/// sets it before it runs.
pub(super) struct PanicAnswer(RefCell<AgentError>);

impl PanicAnswer {
    /// A panic answers `failure` until it is [set](Self::set) again.
    pub(super) fn new(failure: AgentError) -> Self {
        Self(RefCell::new(failure))
    }

    /// What a panic answers now.
    pub(super) fn into_failure(self) -> AgentError {
        self.0.into_inner()
    }

    /// A panic from here on answers `failure`.
    pub(super) fn set(&self, failure: AgentError) {
        *self.0.borrow_mut() = failure;
    }

    fn take(&self) -> AgentError {
        self.0.replace(AgentError::Panicked)
    }
}

/// What a handler serves one request with. Owner thread only.
pub(super) struct ServeContext<'a, T> {
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

impl<T> ServeContext<'_, T> {
    /// The reply, for a handler that answers later. The context keeps a clone, so a panic
    /// after this still answers; the first answer sent is the one taken.
    pub(super) fn park(&self) -> ReplySender<T> {
        self.reply.clone()
    }

    /// Whether an answer of this presentation reported `element`.
    pub(super) fn was_issued(&self, element: flui_protocol::tree::ElementId) -> bool {
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
    fn serve(
        ctx: &mut ServeContext<'_, Self::Output>,
        params: Self::Params,
    ) -> Answer<Self::Output>;
}

/// Runs one row of the table's window methods: a row whose effect is `Annotates` through
/// `run_annotation`, which needs an [`Annotate`] impl, and any other through
/// `run_window_method`, which needs a [`Serve`] impl.
macro_rules! run_row {
    (Annotates, $runtime:expr, $ty:ident, $presentation:ident, $params:ident, $lease:ident, $reply:ident, $report:ident) => {
        $runtime.run_annotation::<method::$ty>($presentation, $params, $reply, $report)
    };
    ($effect:ident, $runtime:expr, $ty:ident, $presentation:ident, $params:ident, $lease:ident, $reply:ident, $report:ident) => {
        $runtime.run_window_method::<method::$ty>($presentation, $params, $lease, $reply, $report)
    };
}

/// Whether this build's windows answer one row of the table's window methods: an annotating
/// row always, since dispatch does not compile without its [`Annotate`] impl, and any other as
/// its [`Serve`] impl says.
macro_rules! answers_row {
    (Annotates, $ty:ident) => {
        true
    };
    ($effect:ident, $ty:ident) => {
        <method::$ty as Serve>::ANSWERS
    };
}

/// How the owner answers a window method that only annotates the window
/// ([`MethodEffect::Annotates`]): from the presentation's annotations alone (module docs,
/// "Annotations").
pub(super) trait Annotate: WindowMethod {
    /// Applies `params` to `banner`: the answer, and whether what the window draws changed.
    fn annotate(
        banner: &BuildBanner,
        params: Self::Params,
    ) -> Result<(Self::Output, bool), AgentError>;
}

impl Annotate for method::BuildReport {
    /// Shows the report; `Succeeded` clears what is shown. A report the window refuses changes
    /// nothing.
    fn annotate(
        banner: &BuildBanner,
        params: BuildReportParams,
    ) -> Result<(Empty, bool), AgentError> {
        match banner.report(params.state) {
            Ok(changed) => Ok((Empty::default(), changed)),
            Err(UnknownBuildState) => Err(AgentError::InvalidArgument {
                reason: "this app's windows do not know that build state",
            }),
        }
    }
}

impl UiRuntime {
    /// Runs the annotating method `M` on `params` in `presentation` and answers `reply`: the
    /// handler is given the presentation's annotations and nothing else, and the frame a change
    /// owes is requested once the answer is sent (module docs, "Annotations").
    fn run_annotation<M: Annotate>(
        &self,
        presentation: PresentationId,
        params: M::Params,
        reply: Reply<M::Output>,
        report: &mut DrainReport,
    ) {
        const {
            assert!(
                matches!(<M as Method>::EFFECT, MethodEffect::Annotates),
                "only a method that annotates is served from the annotations alone"
            );
        };
        let window = super::agent::window_of(presentation);
        let Some(state) = self.presentations.get(presentation) else {
            // Accepted earlier, but the presentation closed first.
            reply.send(Err(AgentError::PresentationGone.into_error(window)));
            report.dropped_stale += 1;
            return;
        };
        // An annotation reports no element handle.
        let reply = ReplySender::new(reply, window, super::agent::issued_of(state), |_, _| {});
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: an annotation runs contained so its caller is answered"
        )]
        let annotated = catch_unwind(AssertUnwindSafe(|| {
            M::annotate(&state.build_banner, params)
        }));
        match annotated {
            Ok(result) => {
                report.invoked += 1;
                let changed = matches!(result, Ok((_, true)));
                send_reply(&reply, None, result.map(|(output, _)| output));
                if changed {
                    // The wake reaches the platform, which must not stand between the request
                    // and its answer.
                    self.request_redraw_for(state);
                }
            }
            Err(payload) => {
                self.fail_agent_job(&reply, None, Err(AgentError::Panicked), payload);
            }
        }
    }

    /// Runs `M` on `params` in `presentation` and answers `reply`: see the module docs.
    fn run_window_method<M: Serve>(
        &self,
        presentation: PresentationId,
        params: M::Params,
        lease: Option<LeaseId>,
        reply: Reply<M::Output>,
        report: &mut DrainReport,
    ) {
        // An annotating method is served from the annotations alone (`run_annotation`), never
        // with the context a `Serve` handler gets.
        const {
            assert!(
                !matches!(<M as Method>::EFFECT, MethodEffect::Annotates),
                "a method that annotates is served by `Annotate`, not `Serve`"
            );
        };
        let window = super::agent::window_of(presentation);
        let Some(state) = self.presentations.get(presentation) else {
            // Accepted earlier, but the presentation closed first.
            reply.send(Err(AgentError::PresentationGone.into_error(window)));
            report.dropped_stale += 1;
            return;
        };
        let issued = super::agent::issued_of(state);
        let mut ctx = ServeContext {
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
        let ServeContext {
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
                            return run_row!(
                                $effect, self, $ty, presentation, params, lease, reply, report
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
                $($name => answers_row!($effect, $ty),)+
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

            fn serve(_: &mut ServeContext<'_, Self::Output>, _: Self::Params) -> Answer<Self::Output> {
                Answer::Now(Err(AgentError::NotSupported {
                    capability: Self::NAME,
                    reason: "this app's windows do not answer it yet",
                }))
            }
        }
    )+};
}

not_answered!(CapturePresented, CaptureRendered,);

impl Serve for method::WindowPlacement {
    /// The surface's logical size, scale and safe area as the presentation's root media query
    /// holds them, the platform whose conventions the runtime installs above its root, and
    /// whether the window is shown and focused as the host last reported. Where the surface
    /// sits on screen is the platform's to say, which this runtime is not told, so `position` is
    /// left out: an OS lane locates the window itself.
    fn serve(ctx: &mut ServeContext<'_, Placement>, _: WindowParams) -> Answer<Placement> {
        let media = ctx.state.media_query.snapshot();
        let size = LogicalSize::new(media.size.width, media.size.height);
        let safe = media.view_padding;
        let placement = Placement::new(ctx.window, size, media.device_pixel_ratio)
            .visible(ctx.state.window_visible.get())
            .focused(ctx.state.window_focused.get())
            .safe_area(LogicalInsets::new(
                safe.top,
                safe.right,
                safe.bottom,
                safe.left,
            ));
        Answer::Now(Ok(match wire_platform(ctx.state.platform) {
            Some(platform) => placement.platform(platform),
            None => placement,
        }))
    }
}

/// The wire's name for the platform whose conventions a presentation follows; `None` for one a
/// later contract adds that the wire does not name yet.
fn wire_platform(platform: flui_platform::TargetPlatform) -> Option<Platform> {
    use flui_platform::TargetPlatform;
    Some(match platform {
        TargetPlatform::Android => Platform::Android,
        TargetPlatform::iOS => Platform::Ios,
        TargetPlatform::Linux => Platform::Linux,
        TargetPlatform::MacOS => Platform::Macos,
        TargetPlatform::Windows => Platform::Windows,
        TargetPlatform::Fuchsia => Platform::Fuchsia,
        TargetPlatform::Unknown => Platform::Neutral,
        _ => return None,
    })
}

impl Serve for method::SemanticsRead {
    fn record(tree: &Tree, issued: &mut IssuedHandles) {
        record_issued(tree, issued);
    }

    fn serve(ctx: &mut ServeContext<'_, Tree>, params: ReadParams) -> Answer<Tree> {
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
    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: InputParams) -> Answer<ActOutcome> {
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
        let script = match agent_input::lower(&params.actions) {
            Ok(script) => script,
            Err(error) => return Answer::Now(Err(error)),
        };
        let before = match agent::tree_before(ctx.state, ctx.window, params.snapshot) {
            Ok(before) => before,
            Err(error) => return Answer::Now(Err(error)),
        };
        let injecting = agent_input::Injecting {
            lease,
            script,
            element: None,
            snapshot: params.snapshot,
            before,
        };
        ctx.runtime
            .queue_injection(ctx.state, injecting, ctx.park());
        Answer::Later
    }
}

impl Serve for method::InputRelease {
    /// Lets go what the call's lease holds once the work queued before it has run.
    fn serve(ctx: &mut ServeContext<'_, Released>, _: WindowParams) -> Answer<Released> {
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

    fn serve(ctx: &mut ServeContext<'_, Matches>, params: LocateParams) -> Answer<Matches> {
        Answer::Now(agent::locate(ctx.state, ctx.window, &params))
    }
}

impl Serve for method::SemanticsAct {
    fn record(outcome: &ActOutcome, issued: &mut IssuedHandles) {
        record_issued_outcome(outcome, issued);
    }

    /// The target is resolved and the action run in this one turn, so the element acted on is
    /// the one the locator matched on the tree the action starts on.
    fn serve(ctx: &mut ServeContext<'_, ActOutcome>, params: ActParams) -> Answer<ActOutcome> {
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
    ctx: &ServeContext<'_, T>,
    locator: &Locator,
) -> Result<flui_protocol::tree::ElementId, AgentError> {
    agent::resolve(ctx.state, ctx.window, locator)
}

impl Serve for method::ElementActionability {
    fn serve(
        ctx: &mut ServeContext<'_, flui_protocol::act::Actionability>,
        params: ActionabilityParams,
    ) -> Answer<flui_protocol::act::Actionability> {
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

    /// It answers itself: now if the condition holds without reading settledness, else after a
    /// frame or turn that makes it hold, or from the host's list after a turn (settledness, its
    /// deadline, the idle end).
    fn serve(ctx: &mut ServeContext<'_, WaitOutcome>, params: WaitParams) -> Answer<WaitOutcome> {
        ctx.if_panics.set(AgentError::Malformed);
        ctx.runtime.park_agent_wait(ctx.state, params, ctx.park());
        Answer::Later
    }
}

impl Serve for method::WaitSettled {
    fn record(outcome: &WaitOutcome, issued: &mut IssuedHandles) {
        record_issued_wait(outcome, issued);
    }

    /// The wait `Condition::Settled`, with the method's timeout: answered from the host's list
    /// after its turn (ADR-0400 §3).
    fn serve(
        ctx: &mut ServeContext<'_, WaitOutcome>,
        params: SettledParams,
    ) -> Answer<WaitOutcome> {
        let mut wait = WaitParams::new(ctx.window, Condition::Settled);
        wait.timeout_ms = params.timeout_ms;
        ctx.if_panics.set(AgentError::Malformed);
        ctx.runtime.park_agent_wait(ctx.state, wait, ctx.park());
        Answer::Later
    }
}

impl Serve for method::WaitFrames {
    fn record(outcome: &WaitOutcome, issued: &mut IssuedHandles) {
        record_issued_wait(outcome, issued);
    }

    /// The wait `Condition::Frames(frames)`, with the method's timeout: it keeps frames coming
    /// and counts those presented after the request.
    fn serve(ctx: &mut ServeContext<'_, WaitOutcome>, params: FramesParams) -> Answer<WaitOutcome> {
        let mut wait = WaitParams::new(ctx.window, Condition::Frames(params.frames));
        wait.timeout_ms = params.timeout_ms;
        ctx.if_panics.set(AgentError::Malformed);
        ctx.runtime.park_agent_wait(ctx.state, wait, ctx.park());
        Answer::Later
    }
}

/// Serves a read of the element or render tree at the owner's turn.
fn inspect_now(
    ctx: &ServeContext<'_, InspectReading>,
    target: InspectTarget,
    params: &InspectParams,
) -> Answer<InspectReading> {
    // A render object's `kind` or an `inspect` override is app code.
    ctx.if_panics.set(AgentError::InspectPanicked);
    Answer::Now(super::agent_inspect::inspect(ctx.state, target, params))
}

impl Serve for method::InspectWidgets {
    fn serve(
        ctx: &mut ServeContext<'_, InspectReading>,
        params: InspectParams,
    ) -> Answer<InspectReading> {
        inspect_now(ctx, InspectTarget::Widgets, &params)
    }
}

impl Serve for method::InspectRender {
    fn serve(
        ctx: &mut ServeContext<'_, InspectReading>,
        params: InspectParams,
    ) -> Answer<InspectReading> {
        inspect_now(ctx, InspectTarget::Render, &params)
    }
}

impl Serve for method::InspectLayers {
    /// Parked for the next scene the window shows.
    fn serve(
        ctx: &mut ServeContext<'_, InspectReading>,
        params: InspectParams,
    ) -> Answer<InspectReading> {
        ctx.runtime
            .park_agent_layer_read(ctx.state, &params, ctx.park());
        Answer::Later
    }
}

impl Serve for method::MotionSet {
    fn serve(ctx: &mut ServeContext<'_, MotionState>, params: MotionParams) -> Answer<MotionState> {
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
        ctx: &mut ServeContext<'_, AnnouncementJournal>,
        params: AnnouncementsParams,
    ) -> Answer<AnnouncementJournal> {
        Answer::Now(Ok(ctx.state.pipeline().with(|pipeline| {
            pipeline
                .semantics_owner()
                .map_or_default(|owner| owner.announcement_journal(&params))
        })))
    }
}

impl Serve for method::InputJournal {
    fn serve(ctx: &mut ServeContext<'_, Journal>, params: JournalParams) -> Answer<Journal> {
        Answer::Now(Ok(agent_journal::read(ctx.state, &params)))
    }
}
