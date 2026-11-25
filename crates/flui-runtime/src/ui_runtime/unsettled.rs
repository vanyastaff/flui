//! The runtime's half of what keeps an application from being idle, and the answers to the waits
//! that read it (ADR-0400 §3).

use flui_foundation::PresentationId;
use flui_protocol::wait::{SettleScope, UnsettledReason};
use web_time::Instant;

use super::UiRuntime;
use crate::presentation::PresentationState;
use crate::settledness::{HostHalf, Unsettled};

impl UiRuntime {
    /// Lists into `into` what keeps this runtime from being idle at the instant `into` was
    /// gathered at ([`Unsettled::at`], set by [`Unsettled::new`]), each cause with what
    /// discharges it as the runtime's own state says:
    ///
    /// - a redraw (`NeedsRedraw`), a scheduled frame (`FrameScheduled`), work a frame takes
    ///   (`PendingWork`: tree work, due work, pointer motion to resample) and each presentation's
    ///   running animation (`Ticker`, with its window, whether or not what it animates is painted)
    ///   are the next turn's, but only while the runtime's frames are on: a runtime that sees no
    ///   window runs no frame for them, and lists none. A cause of a presentation whose dropped
    ///   frame's retry is ahead is served at that retry, and listed at it
    ///   ([`Self::list_for_next_turn`]); another presentation's retry holds nothing of it. A
    ///   redraw, a scheduled frame and tree work or motion are listed with the window of each
    ///   presentation whose work the next frame serves (or that has motion, which every frame
    ///   flushes); a scheduled frame no presentation's work asks for is listed with none, as is
    ///   due work. A presentation whose work no frame serves (one that cannot be seen, whose sink
    ///   is full or whose device was given up) is named for none of them, whether or not another
    ///   window's frames run, and demand it alone holds is not listed, as with frames off: what
    ///   serves its work is its window being seen, the frame its window is asked for (the host's
    ///   half), the settlement the in-flight floor bounds (a `Timer`) or a rebuild;
    /// - a task woken on the owner's driver is the application's own work, which the next turn
    ///   polls whatever any window holds: listed with no window as the frame it scheduled
    ///   (`FrameScheduled`) while frames are on, and as `PendingWork` while they are off, when a
    ///   background turn polls it;
    /// - every deadline the runtime wakes for (a gesture's, its geometry's, text preparation's, a
    ///   haptic delivery's, a frame-rate hold's, a dropped frame's retry, the in-flight floor
    ///   where the host owes it, a task's sleep, a drag's spring loading) is a `Timer` served at
    ///   its exact instant, with the window of the presentation it belongs to: the enumeration
    ///   and the floor rule [`Self::next_wake`] reads;
    /// - what the development agent asks for itself is never a cause: the deadline of a wait it
    ///   parked or an act it holds is a wake at that instant, and the frames it asks for (a held
    ///   act's boundaries, a wait that counts frames) a wake at once. Input an act queued is the
    ///   application's work, listed as a redraw of its window until it has played;
    /// - a command or an owner job queued for the next turn is `PendingWork` of the whole
    ///   application, frames on or off.
    ///
    /// A frame a covered window owes its surface is its host's half, never listed here. Each
    /// cause is read at [`Unsettled::at`], the instant `into` was gathered at. Reads state and
    /// runs no user code.
    pub fn unsettled(&self, into: &mut Unsettled) {
        let frames_enabled = self.scheduler.frames_enabled();
        let task_woken = self.owner_frame.ready_task_count() != 0;
        if frames_enabled {
            // A redraw flag no presentation's work stands behind is a wake, not work: the
            // platform's, or a frame the agent asked for.
            if self.needs_redraw() {
                let _ = self.list_frame_work(into, UnsettledReason::NeedsRedraw, &|_| false);
            }
            // A scheduled frame no presentation's work stands behind is the application's own
            // (a woken task's is listed below, whatever the windows hold).
            if self.scheduler.is_frame_scheduled()
                && !self.list_frame_work(into, UnsettledReason::FrameScheduled, &|_| false)
                && !task_woken
            {
                self.list_for_next_turn(into, UnsettledReason::FrameScheduled, None);
            }
            // A gesture deadline armed and not yet come is its own `Timer` below: a frame before
            // it serves nothing.
            let has_motion =
                |presentation: &PresentationState| presentation.gestures().has_pending_motion();
            if self.has_pending_due_work() {
                // Due work is the runtime's (a task's deadline) or a presentation's (a retry, a
                // gesture's deadline, a haptic delivery, a hold); it is named for none.
                self.list_for_next_turn(into, UnsettledReason::PendingWork, None);
            } else if self.has_pending_tree_work() || self.presentations.iter().any(has_motion) {
                let _ = self.list_frame_work(into, UnsettledReason::PendingWork, &has_motion);
            }
            for presentation in self.presentations.iter() {
                if !presentation.closing_requested.get()
                    && presentation.animation_running(frames_enabled)
                {
                    self.list_for_next_turn(into, UnsettledReason::Ticker, Some(presentation.id()));
                }
            }
            if self.agent_frame_asked() {
                into.wake_at_once();
            }
        }
        // A woken task is polled by the next turn, a frame's begin or a background turn alike: a
        // window's frame work, served or not, holds nothing of it.
        if task_woken {
            let reason = if frames_enabled {
                UnsettledReason::FrameScheduled
            } else {
                UnsettledReason::PendingWork
            };
            self.list_for_next_turn(into, reason, None);
        }
        // A command or an owner job queued for the next turn (a signal written off the owner
        // thread, a capability's completion, a request a client sent) is drained by that turn,
        // a frame or a background turn alike.
        if self.has_queued_work() {
            self.list_for_next_turn(into, UnsettledReason::PendingWork, None);
        }
        self.visit_armed_deadlines(&mut |presentation, at| {
            into.deadline(
                UnsettledReason::Timer,
                presentation.map(super::agent::window_of),
                at,
            );
        });
        if let Some(at) = self.next_agent_deadline() {
            into.wake_at(at);
        }
    }

    /// Lists `reason`, of `presentation`'s window or of the whole application, as the next turn
    /// serves it: at that presentation's dropped-frame retry while one is ahead and a turn at it
    /// serves it ([`PresentationState::retry_served_at`]), since the runtime attempts no frame of
    /// it sooner, and at the next turn otherwise. A retry holds only its own presentation's
    /// causes: another window's frames, and the application's own work, run at the next turn.
    /// The one rule for the runtime's half and its host's, which lists its own causes of a
    /// window, and of the whole application, through it.
    pub fn list_for_next_turn(
        &self,
        into: &mut Unsettled,
        reason: UnsettledReason,
        presentation: Option<PresentationId>,
    ) {
        let now = into.at();
        let window = presentation.map(super::agent::window_of);
        let retry = presentation
            .and_then(|id| self.presentations.get(id))
            .and_then(PresentationState::retry_served_at)
            .filter(|retry| *retry > now);
        match retry {
            Some(retry) => into.deadline(reason, window, retry),
            None => into.next_frame(reason, window),
        }
    }

    /// Lists `reason`, a runtime-wide demand for a frame, once with the window of each
    /// presentation whose work the next frame serves (or for which `also` holds), so a
    /// `wait.settled` on a window another window's demand does not touch is not held by it
    /// (ADR-0400 §3). Where only presentations no frame serves hold frame work, that work waits
    /// for what serves it and the demand is not listed, as with frames off. Returns whether any
    /// presentation stands behind the demand, served or not: where none does, the demand is the
    /// application's own, which the caller lists with no window.
    fn list_frame_work(
        &self,
        into: &mut Unsettled,
        reason: UnsettledReason,
        also: &dyn Fn(&PresentationState) -> bool,
    ) -> bool {
        let mut stood_behind = false;
        for presentation in self.presentations.iter() {
            let served = presentation.has_frame_work_for_next_frame() || also(presentation);
            if served {
                self.list_for_next_turn(into, reason, Some(presentation.id()));
            }
            stood_behind |= served || presentation.has_frame_work();
        }
        stood_behind
    }

    /// The host, after its turn and its own steps after it: answers from `host` every parked wait
    /// whose answer reads settledness. A wait whose condition contains `Condition::Settled` is
    /// answered here alone, as is every wait's timeout and the idle end; a wait that holds without
    /// reading settledness is also answered at the frame or turn that makes it hold.
    ///
    /// On [`HostHalf::Complete`] a wait counts both halves (`SettleScope::Complete`), and one
    /// whose condition is false ends idle when every cause listed, in any window, is one only
    /// the test discharges (none at all included: nothing but the test can change the
    /// application then). On [`HostHalf::Absent`] it counts the runtime's half
    /// ([`Self::unsettled`]) alone (`SettleScope::RuntimeOnly`) and never ends idle. A
    /// `wait.settled` on a window counts that window's causes and the application's, not
    /// another window's. A wait answered [`TURNS_AT_ONE_INSTANT`] times at one instant ends
    /// unsatisfied, naming the causes due then.
    ///
    /// A runtime whose host does not compose a half
    /// ([`RuntimeHostServices::composes_host_half`](super::RuntimeHostServices::composes_host_half))
    /// answers with [`HostHalf::Absent`] itself wherever its outermost turn ends: an owner
    /// dispatch, a turn its host ran outside one, and a frame settlement ([`Self::settle_frame`])
    /// outside any turn, which discharges a cause with no turn after it.
    ///
    /// # Panics
    ///
    /// With the first panic of a wait's evaluation or of a reply's delivery, once every wait was
    /// answered or put back.
    ///
    /// [`TURNS_AT_ONE_INSTANT`]: crate::settledness::TURNS_AT_ONE_INSTANT
    pub fn answer_settled(&mut self, now: Instant, host: HostHalf<'_>) {
        self.answer_settled_waits(now, host);
    }

    /// Whether a wait an agent parked is still unanswered: what [`Self::answer_settled`] would
    /// answer. A host gathers its list for that call only while this holds, so a turn nobody
    /// waits on costs it nothing. Read through the agent's owner side, so a binary with no
    /// agent reaches none of it (ADR-0402).
    #[must_use]
    pub fn awaits_settled_answer(&self) -> bool {
        self.agent_side
            .get()
            .is_some_and(|agent| agent.awaits_settled_answer(self))
    }

    /// A runtime whose host composes no half answers its settled waits from its own, wherever
    /// its outermost turn ends: an owner dispatch ([`crate::owner`]), a turn a host ran by
    /// itself, and a frame's settlement outside any turn, which discharges the frame in flight
    /// with no turn after it. It does nothing while a turn is still open, whose end answers, and
    /// nothing on a host that composes a half: that host answers after its own steps
    /// (ADR-0400 §3). Everything past those checks runs through the agent's owner side, so a
    /// binary with no agent keeps none of the runtime's half (ADR-0402).
    pub(crate) fn answer_settled_from_own_half(&self) {
        if self.composes_host_half || self.geometry_turn.borrow().is_some() {
            return;
        }
        let Some(agent) = self.agent_side.get() else {
            return;
        };
        if agent.awaits_settled_answer(self) {
            let now = flui_foundation::MonotonicClock::now(&self.clock);
            self.answer_settled_waits(now, HostHalf::Absent);
        }
    }

    /// [`Self::answer_settled`] behind `&self`, for the runtime's own answers.
    fn answer_settled_waits(&self, now: Instant, host: HostHalf<'_>) {
        let Some(agent) = self.agent_side.get() else {
            return;
        };
        match host {
            HostHalf::Complete(unsettled) => {
                agent.answer_settled(self, unsettled, SettleScope::Complete);
            }
            HostHalf::Absent => agent.answer_from_runtime_half(self, now),
        }
    }

    /// The runtime's half at `now`, answering every parked wait that reads settledness with
    /// `SettleScope::RuntimeOnly`: what [`HostHalf::Absent`] answers. Reached only through the
    /// agent's owner side (`AgentSide::answer_from_runtime_half`).
    pub(super) fn answer_settled_agent_waits_from_own_half(&self, now: Instant) {
        let mut unsettled = Unsettled::new(now);
        self.unsettled(&mut unsettled);
        self.answer_settled_agent_waits(&unsettled, SettleScope::RuntimeOnly);
    }

    /// Which halves an answer of this runtime counts: both where its host composes one.
    pub(super) fn settle_scope(&self) -> SettleScope {
        if self.composes_host_half {
            SettleScope::Complete
        } else {
            SettleScope::RuntimeOnly
        }
    }
}
