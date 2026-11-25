//! The acts an agent asked a window for that hold out at its frame boundaries (ADR-0400 §2): a
//! gesture waiting for its target to turn actionable, a scroll searching for an element.
//!
//! A presentation's [`HeldActs`] is the one owner of every act it holds. An act is attempted when
//! it arrives, then at each frame boundary ([`UiRuntime::advance_held_acts`], which the frame path
//! reaches through `AgentSide`), and at an owner turn without a frame once its deadline has
//! passed, so a window that draws nothing still answers it. While any act is held the window asks
//! for frames: stability is a fact about two frames.
//!
//! The deadline is the act's own timeout, on the presentation's clock, and it wakes the owner
//! ([`UiRuntime::next_held_act_deadline`]). An attempt at or past it is the last: it performs the
//! act if the target passed on that frame, and otherwise answers why it never did. The act is
//! dropped as it is answered, so nothing it would have sent goes out after its answer.
//!
//! Every exit answers once and leaves nothing held: performed (the act hands its reply to what
//! answers it: the injection, or the reading a semantics action owes), refused, timed out, the
//! presentation closing (`gone`), the lease it injects through let go (`gone`), and a panic in app
//! code an attempt reached (the reply is sent before the panic continues). An act nobody awaits
//! any more is dropped unanswered, as a wait is. A presentation holds at most [`MAX_HELD_ACTS`].

use flui_foundation::panic::FirstPanic;
use flui_protocol::{act::ActOutcome, tree::WindowId};
use flui_view::__runtime::LeaseId;
use web_time::Instant;

use super::UiRuntime;
use super::agent::{AgentError, ReplySender, send_reply, window_of};
use super::agent_gesture::GestureAct;
use super::agent_scroll::ScrollSearch;
use super::agent_serve::PanicAnswer;
use crate::presentation::PresentationState;

/// The most acts one presentation holds at once. An act past it is refused with
/// [`AgentError::TooManyActs`], never dropped silently or in place of an older one.
pub(crate) const MAX_HELD_ACTS: usize = 64;

/// The acts one presentation holds. Owner thread only.
#[derive(Default)]
pub(crate) struct HeldActs {
    acts: Vec<HeldAct>,
}

/// One held act: what it holds out for, until when, and where its answer goes.
pub(crate) struct HeldAct {
    /// On the presentation's clock.
    deadline: Instant,
    /// The input lease its input goes through: letting it go drops the act.
    lease: Option<LeaseId>,
    reply: ReplySender<ActOutcome>,
    holding: Holding,
}

/// What a held act holds out for.
pub(super) enum Holding {
    /// A gesture, for its target to turn actionable.
    Gesture(GestureAct),
    /// `element.scroll_until`, for its element to show under the scrollable.
    Scroll(ScrollSearch),
}

/// What one attempt at a held act came to.
pub(super) enum Attempt {
    /// Not yet: the act holds on to the next attempt.
    Hold,
    /// Answered, or handed to what answers it.
    Done,
}

/// What an attempt works with: the window, its clock, and where a panic's answer and a caught
/// panic go.
pub(super) struct AttemptContext<'a> {
    pub(super) runtime: &'a UiRuntime,
    pub(super) state: &'a PresentationState,
    pub(super) window: WindowId,
    /// The presentation's clock, now.
    pub(super) now: Instant,
    /// The act's deadline, which the input it queues carries.
    pub(super) deadline: Instant,
    /// The deadline has passed: the attempt is the last.
    pub(super) last: bool,
    /// The input lease the act injects through.
    pub(super) lease: Option<LeaseId>,
    /// What a panic answers, as far as the attempt has got.
    pub(super) if_panics: &'a PanicAnswer,
    /// A panic a step caught and handed back instead of unwinding (the hit-test's): resumed once
    /// the act is answered.
    pub(super) caught: &'a mut FirstPanic,
}

impl HeldAct {
    /// An act holding out for `holding` until `deadline`, injecting through `lease`, answered at
    /// `reply`.
    pub(super) fn new(
        deadline: Instant,
        lease: Option<LeaseId>,
        holding: Holding,
        reply: ReplySender<ActOutcome>,
    ) -> Self {
        Self {
            deadline,
            lease,
            reply,
            holding,
        }
    }

    /// Whether anyone can still take the answer.
    fn is_awaited(&self) -> bool {
        self.reply.is_awaited()
    }

    /// The window closed before the act ended.
    pub(crate) fn abandon(self) {
        send_reply(&self.reply, None, Err(AgentError::PresentationGone));
    }

    /// The lease the act injects through was let go before the act ended.
    pub(crate) fn lease_released(self) {
        send_reply(&self.reply, None, Err(AgentError::LeaseReleased));
    }

    /// One attempt in `ctx`'s window: the act back while it holds on.
    fn attempt(mut self, ctx: &mut AttemptContext<'_>) -> Option<Self> {
        ctx.lease = self.lease;
        ctx.deadline = self.deadline;
        ctx.last = ctx.now >= self.deadline;
        let attempt = match &mut self.holding {
            Holding::Gesture(gesture) => gesture.attempt(ctx, &self.reply),
            Holding::Scroll(search) => search.attempt(ctx, &self.reply),
        };
        match attempt {
            Attempt::Hold => Some(self),
            Attempt::Done => None,
        }
    }
}

impl HeldActs {
    /// Every act, to be answered that the window is gone.
    pub(crate) fn abandon(&mut self) -> Vec<HeldAct> {
        std::mem::take(&mut self.acts)
    }

    /// The acts that inject through one of `leases`, which were let go, to be answered so.
    pub(super) fn forget(&mut self, leases: &[LeaseId]) -> Vec<HeldAct> {
        let (dropped, kept) = std::mem::take(&mut self.acts)
            .into_iter()
            .partition(|act| act.lease.is_some_and(|lease| leases.contains(&lease)));
        self.acts = kept;
        dropped
    }

    /// The earliest deadline that wakes the owner: every act's, a scroll's whose step is under
    /// way included, since the pass at the deadline answers it whether or not a frame comes.
    fn next_deadline(&self) -> Option<Instant> {
        self.acts.iter().map(|act| act.deadline).min()
    }
}

/// Which held acts an attempt pass reaches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pass {
    /// A frame boundary: every act.
    Boundary,
    /// An owner turn without a frame: the acts whose deadline has passed.
    Expired,
}

impl UiRuntime {
    /// Hold `act` in `state`'s window: refused when the window holds [`MAX_HELD_ACTS`] already,
    /// before anything of it runs; otherwise attempted at once, in the serving turn, and held
    /// when it holds on. A panic in the attempt continues into the serving turn's containment,
    /// which answers `if_panics`; one the hit-test caught is left in `caught`.
    ///
    /// The frames a held act asks for are asked for before the attempt: the wake reaches the
    /// platform, user code, and one that panics has held and queued nothing, so the serving
    /// turn's answer to it is true. Nothing that runs code this module does not own comes after
    /// the act is held.
    pub(super) fn hold_act(
        &self,
        state: &PresentationState,
        act: HeldAct,
        if_panics: &PanicAnswer,
        caught: &mut FirstPanic,
    ) {
        // Acts nobody takes any more hold no place against the limit.
        let held = {
            let mut acts = state.agent_acts.borrow_mut();
            acts.acts.retain(HeldAct::is_awaited);
            acts.acts.len()
        };
        if held >= MAX_HELD_ACTS {
            send_reply(&act.reply, None, Err(AgentError::TooManyActs));
            return;
        }
        if_panics.set(AgentError::Panicked);
        self.request_agent_frame_for(state);
        let now = state.clock().now();
        let mut ctx = AttemptContext {
            runtime: self,
            state,
            window: window_of(state.id()),
            now,
            deadline: now,
            last: false,
            lease: None,
            if_panics,
            caught,
        };
        if let Some(act) = act.attempt(&mut ctx) {
            state.agent_acts.borrow_mut().acts.push(act);
        }
    }

    /// Attempt the held acts `pass` reaches in every window. An attempt that panics in app code
    /// answers its own act; the answer and each window's wake run contained, and the first panic
    /// of any of them continues once every window's acts are put back.
    pub(super) fn advance_held_acts(&self, pass: Pass) {
        let mut first = FirstPanic::new();
        for state in self.presentations.iter() {
            if state.agent_acts.borrow().acts.is_empty() {
                continue;
            }
            let now = state.clock().now();
            let acts = std::mem::take(&mut state.agent_acts.borrow_mut().acts);
            let mut keep = Vec::with_capacity(acts.len());
            for act in acts {
                // Nobody takes this answer any more: the act goes, and nothing of it is sent.
                if !act.is_awaited() {
                    continue;
                }
                if pass == Pass::Expired && now < act.deadline {
                    keep.push(act);
                    continue;
                }
                let reply = act.reply.clone();
                let if_panics = PanicAnswer::new(AgentError::Panicked);
                let mut caught = FirstPanic::new();
                let mut attempted = FirstPanic::new();
                let kept = attempted.contain(|| {
                    let mut ctx = AttemptContext {
                        runtime: self,
                        state,
                        window: window_of(state.id()),
                        now,
                        deadline: now,
                        last: false,
                        lease: None,
                        if_panics: &if_panics,
                        caught: &mut caught,
                    };
                    act.attempt(&mut ctx)
                });
                first.absorb(caught);
                first.absorb(attempted);
                match kept {
                    Some(Some(act)) => keep.push(act),
                    Some(None) => {}
                    // The attempt consumed the act; the reply kept apart answers it, behind the
                    // attempt's panic.
                    None => {
                        let failure = if_panics.into_failure();
                        first.contain(|| send_reply(&reply, None, Err(failure)));
                    }
                }
            }
            // Acts held while this ran (none can be, on the owner) stay behind the old ones.
            let holding = {
                let mut held = state.agent_acts.borrow_mut();
                keep.append(&mut held.acts);
                held.acts = keep;
                !held.acts.is_empty()
            };
            if holding {
                first.contain(|| self.request_agent_frame_for(state));
            }
        }
        first.resume();
    }

    /// The earliest deadline of an act on a window that is not closing, on the presentations'
    /// clocks: a held act's, and that of the input an act queued that has not played yet
    /// ([`UiRuntime::expire_act_input`]), so an act is answered by it whether or not its window
    /// draws.
    pub(super) fn next_held_act_deadline(&self) -> Option<Instant> {
        self.presentations
            .iter()
            .filter(|presentation| !presentation.closing_requested.get())
            .filter_map(|presentation| {
                let held = presentation.agent_acts.borrow().next_deadline();
                let queued = Self::next_act_input_deadline(presentation);
                held.into_iter().chain(queued).min()
            })
            .min()
    }
}
