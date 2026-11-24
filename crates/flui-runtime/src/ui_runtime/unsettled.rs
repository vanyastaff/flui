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
    ///   not yet due they are served at the retry instead, and listed at it;
    /// - every deadline the runtime wakes for (a gesture's, its geometry's, text preparation's, a
    ///   haptic delivery's, a frame-rate hold's, a dropped frame's retry, the in-flight floor, a
    ///   task's sleep, a drag's spring loading) is a `Timer` served at its exact instant, with the
    ///   window of the presentation it belongs to;
    /// - a wait an agent parked is never a cause: its deadline is a wake alone.
    ///
    /// A frame a covered window owes its surface is its host's half, never listed here. Reads
    /// state and runs no user code.
    pub fn unsettled(&self, now: Instant, into: &mut Unsettled) {
        let held_until = self.failure_retry_deadline().filter(|retry| *retry > now);
        let next_turn = |into: &mut Unsettled, reason, window| match held_until {
            Some(retry) => into.deadline(reason, window, retry),
            None => into.next_frame(reason, window),
        };
        let frames_enabled = self.scheduler.frames_enabled();
        if frames_enabled {
            if self.needs_redraw() {
                next_turn(into, UnsettledReason::NeedsRedraw, None);
            }
            if self.scheduler.is_frame_scheduled() {
                next_turn(into, UnsettledReason::FrameScheduled, None);
            }
            // A gesture deadline armed and not yet come is its own `Timer` below: a frame before
            // it serves nothing.
            let motion = self
                .presentations
                .iter()
                .any(|presentation| presentation.gestures().has_pending_motion());
            if self.has_pending_tree_work() || self.has_pending_due_work() || motion {
                next_turn(into, UnsettledReason::PendingWork, None);
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
    /// answers with [`HostHalf::Absent`] itself at the end of each turn.
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

    /// [`Self::answer_settled`], for the runtime's own turn end.
    pub(super) fn answer_settled_waits(&self, now: Instant, host: HostHalf<'_>) {
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

    /// Whether the host answers settledness after each of its turns.
    pub(super) fn host_composes_half(&self) -> bool {
        self.composes_host_half
    }
}
