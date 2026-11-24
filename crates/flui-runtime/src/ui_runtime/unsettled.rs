//! The runtime's half of what keeps an application from being idle, and the answers to the waits
//! that read it (ADR-0400 §3).

use flui_protocol::wait::{SettleScope, UnsettledReason};
use web_time::Instant;

use super::UiRuntime;
use crate::settledness::{HostHalf, Unsettled};

impl UiRuntime {
    /// Lists into `into` what keeps this runtime from being idle at `now`, each cause with what
    /// discharges it as the runtime's own state says:
    ///
    /// - a redraw (`NeedsRedraw`), a scheduled frame (`FrameScheduled`), work a frame takes
    ///   (`PendingWork`: tree work, due work, pointer motion to resample) and each presentation's
    ///   running animation (`Ticker`, with its window, whether or not what it animates is painted)
    ///   are the next turn's, but only while the runtime's frames are on: a runtime that sees no
    ///   window runs no frame for them, and lists none. While a dropped frame's retry is armed and
    ///   not yet due they are served at the retry instead, and listed at it. A redraw, a
    ///   scheduled frame and tree work or motion are listed with the window of each presentation
    ///   that has work for a frame (or motion), and with none when no presentation has any; due
    ///   work is listed with none;
    /// - every deadline the runtime wakes for (a gesture's, its geometry's, text preparation's, a
    ///   haptic delivery's, a frame-rate hold's, a dropped frame's retry, the in-flight floor, a
    ///   task's sleep, a drag's spring loading) is a `Timer` served at its exact instant, with the
    ///   window of the presentation it belongs to;
    /// - a wait an agent parked or an act it holds is never a cause: its deadline is a wake alone.
    ///
    /// A frame a covered window owes its surface is its host's half, never listed here. Reads
    /// state and runs no user code.
    pub fn unsettled(&self, now: Instant, into: &mut Unsettled) {
        let held_until = self.failure_retry_deadline().filter(|retry| *retry > now);
        let next_turn = |into: &mut Unsettled, reason, window| match held_until {
            Some(retry) => into.deadline(reason, window, retry),
            None => into.next_frame(reason, window),
        };
        // The runtime-wide demand for a frame changes only the presentations with work for it:
        // it is listed once with the window of each, or with none when no presentation has any,
        // so a `wait.settled` on a window another window's demand does not touch is not held by
        // it (ADR-0400 §3).
        let for_frame_work =
            |into: &mut Unsettled,
             reason,
             also: &dyn Fn(&crate::presentation::PresentationState) -> bool| {
                let mut named = false;
                for presentation in self.presentations.iter() {
                    if presentation.has_frame_work() || also(presentation) {
                        next_turn(
                            into,
                            reason,
                            Some(super::agent::window_of(presentation.id())),
                        );
                        named = true;
                    }
                }
                if !named {
                    next_turn(into, reason, None);
                }
            };
        let frames_enabled = self.scheduler.frames_enabled();
        if frames_enabled {
            if self.needs_redraw() {
                for_frame_work(into, UnsettledReason::NeedsRedraw, &|_| false);
            }
            if self.scheduler.is_frame_scheduled() {
                for_frame_work(into, UnsettledReason::FrameScheduled, &|_| false);
            }
            // A gesture deadline armed and not yet come is its own `Timer` below: a frame before
            // it serves nothing.
            let has_motion = |presentation: &crate::presentation::PresentationState| {
                presentation.gestures().has_pending_motion()
            };
            if self.has_pending_due_work() {
                // Due work is the runtime's (a task's deadline) or a presentation's (a retry, a
                // gesture's deadline, a haptic delivery, a hold); it is named for none.
                next_turn(into, UnsettledReason::PendingWork, None);
            } else if self.has_pending_tree_work()
                || self
                    .presentations
                    .iter()
                    .any(|presentation| has_motion(presentation))
            {
                for_frame_work(into, UnsettledReason::PendingWork, &has_motion);
            }
            for presentation in self.presentations.iter() {
                if !presentation.closing_requested.get()
                    && presentation.animation_running(frames_enabled)
                {
                    next_turn(
                        into,
                        UnsettledReason::Ticker,
                        Some(super::agent::window_of(presentation.id())),
                    );
                }
            }
        }
        self.visit_armed_deadlines(&|_| true, &mut |presentation, at| {
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
    /// waits on costs it nothing.
    #[must_use]
    pub fn awaits_settled_answer(&self) -> bool {
        self.agent_side.get().is_some()
            && self
                .presentations
                .iter()
                .any(|presentation| !presentation.agent_waits.borrow().is_empty())
    }

    /// A runtime whose host composes no half answers its settled waits from its own, wherever
    /// its outermost turn ends: an owner dispatch ([`crate::owner`]), a turn a host ran by
    /// itself, and a frame's settlement outside any turn, which discharges the frame in flight
    /// with no turn after it. It does nothing while a turn is still open, whose end answers, and
    /// nothing on a host that composes a half: that host answers after its own steps
    /// (ADR-0400 §3).
    pub(crate) fn answer_settled_from_own_half(&self) {
        if self.composes_host_half
            || self.geometry_turn.borrow().is_some()
            || !self.awaits_settled_answer()
        {
            return;
        }
        let now = flui_foundation::MonotonicClock::now(&self.clock);
        self.answer_settled_waits(now, HostHalf::Absent);
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
            HostHalf::Absent => {
                let mut unsettled = Unsettled::new(now);
                self.unsettled(now, &mut unsettled);
                agent.answer_settled(self, &unsettled, SettleScope::RuntimeOnly);
            }
        }
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
