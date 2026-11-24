//! How a [`HeadlessHost`] decides it has settled: the one list of what keeps the
//! application from being idle (ADR-0400 §3), the loop that runs turns for a
//! wait, background work and text sizing.

#[cfg(doc)]
use super::HeadlessWindow;

use std::time::Duration;

use flui_foundation::PresentationId;
use flui_protocol::error::ErrorObject;
use flui_protocol::tree::WindowId;
use flui_runtime::pump::FrameOutcome;
use flui_runtime::settledness::{HostHalf, Unsettled};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::dev_agent::AgentAnswer;

use super::errors::NotSettled;
use super::text_sizing::{self, ScriptedTextSizing};
use super::{HeadlessHost, HeadlessWindowId};

impl HeadlessHost {
    /// What keeps the application from being idle now: the UI runtime's half
    /// ([`UiRuntime::unsettled`]) and the host's own, gathered into one list
    /// (ADR-0400 §3). [`time_to_next_frame`](Self::time_to_next_frame) is its
    /// [wake](Unsettled::wake), [`pump_until_settled`](Self::pump_until_settled)
    /// settles when it is empty, and each [`pump`](Self::pump) hands it to the
    /// runtime's agent waits once the pump's own steps are done.
    ///
    /// Not a pure read: it first asks each covered window for the frame it
    /// owes, as a runner's owner delivery does after the work item that left
    /// it owed, which runs the windows' `request_frame_at` and records the
    /// request ([`HeadlessWindow::timed_frame_requests`]). Asking again with
    /// nothing changed asks nothing.
    #[must_use]
    pub fn unsettled(&self) -> Unsettled {
        // What changed since the last turn (a signal written between pumps)
        // may leave a covered window owing a frame: its window is asked first,
        // as a runner's owner delivery asks at the end of the work item that
        // changed it, and the list counts the request.
        self.ask_owed_frames();
        self.gather_unsettled()
    }

    /// The host's list, its owed frames already asked. Its own half:
    ///
    /// - background work (`Background`): held work only the test releases,
    ///   runnable work the next turn runs, and a parked future something
    ///   outside the application wakes;
    /// - a turn owed for a close or a quit (`OwedTurn`), and a window
    ///   observation not yet delivered (`WindowObservation`), each the next
    ///   turn's;
    /// - a window's timed frame request (ADR-0389), served by a turn at its
    ///   instant (`FrameRequest`) and, once that instant has come, the frame a
    ///   covered window owes its surface, which the next turn serves
    ///   (`SurfaceFrame`);
    /// - the scenario's next change (`ScenarioStep`), at its instant.
    ///
    /// While a dropped frame's retry is armed and not yet due the runtime runs
    /// no frame sooner, and the host's own next-turn causes (background work,
    /// a turn owed, an observation) wait for it too, listed at the retry; the
    /// due frame request and the scenario's step are served at their own
    /// instants.
    fn gather_unsettled(&self) -> Unsettled {
        use flui_protocol::wait::UnsettledReason;

        let now = flui_foundation::MonotonicClock::now(&self.clock);
        let mut unsettled = Unsettled::new(now);
        let held_until = self.ui_runtime.as_ref().and_then(|runtime| {
            runtime.unsettled(now, &mut unsettled);
            runtime
                .failure_retry_deadline()
                .filter(|retry| *retry > now)
        });
        let next_turn =
            |unsettled: &mut Unsettled, reason, window: Option<WindowId>| match held_until {
                Some(retry) => unsettled.deadline(reason, window, retry),
                None => unsettled.next_frame(reason, window),
            };

        let background = &self.scripted.background;
        let pending = background.pending();
        if background.held.get() {
            if pending > 0 {
                unsettled.test(UnsettledReason::Background, None);
            }
        } else {
            let runnable = background.executors.runnable();
            if runnable > 0 {
                next_turn(&mut unsettled, UnsettledReason::Background, None);
            }
            if pending > runnable {
                unsettled.outside(UnsettledReason::Background, None);
            }
        }
        if self.turn_owed() {
            next_turn(&mut unsettled, UnsettledReason::OwedTurn, None);
        }
        let epoch = flui_foundation::MonotonicClock::epoch(&self.clock);
        for open in &self.open {
            let window = Some(wire_window(open.id));
            if open.window.chrome.borrow().unobserved {
                next_turn(&mut unsettled, UnsettledReason::WindowObservation, window);
            }
            if let Some(at) = open.window.pending_timed_frame() {
                let at = epoch
                    .checked_add(Duration::from_nanos(at.as_nanos()))
                    .unwrap_or(epoch);
                if at <= now {
                    unsettled.next_frame(UnsettledReason::SurfaceFrame, window);
                } else {
                    unsettled.deadline(UnsettledReason::FrameRequest, window, at);
                }
            }
        }
        let scenario = self.scripted.clock.scenario();
        if let Some(due) = scenario.next_due() {
            let at = now
                .checked_add(due.saturating_sub(scenario.now()))
                .unwrap_or(now);
            unsettled.deadline(UnsettledReason::ScenarioStep, None, at);
        }
        unsettled
    }

    /// After a pump's own steps: hands the list to the UI runtime, which
    /// answers its agent's settled waits, timeouts and idle ends from it. The
    /// list is gathered only while a wait is parked, so a pump nobody waits
    /// on allocates nothing for it.
    pub(super) fn answer_settled(&mut self) {
        if !self
            .ui_runtime
            .as_ref()
            .is_some_and(UiRuntime::awaits_settled_answer)
        {
            return;
        }
        let unsettled = self.gather_unsettled();
        if let Some(runtime) = self.ui_runtime.as_mut() {
            runtime.answer_settled(unsettled.at(), HostHalf::Complete(&unsettled));
        }
    }

    /// How long a host's loop waits before its next frame: the list's
    /// [wake](Unsettled::wake), from now ([`unsettled`](Self::unsettled), which
    /// asks each covered window for the frame it owes first). `None` when the
    /// loop would wait for input alone.
    #[must_use]
    pub fn time_to_next_frame(&self) -> Option<Duration> {
        let unsettled = self.unsettled();
        unsettled
            .wake()
            .map(|wake| wake.saturating_duration_since(unsettled.at()))
    }

    /// Run turns for a request `answer` waits on, on the host's virtual clock,
    /// until it is answered: each turn at the list's wake, the application's
    /// timers included, so a wait's deadline is reached in virtual time. A
    /// cause the next turn serves is served at once; one that re-armed at that
    /// instant is served a `frame` later, as a display paces its frames, unless
    /// a deadline is due, which is served at once again. Stops unanswered,
    /// returning `None`, once `limit` of virtual time has passed, once nothing
    /// is left to wake for, or after
    /// [`TURNS_AT_ONE_INSTANT`](flui_runtime::settledness::TURNS_AT_ONE_INSTANT)
    /// turns without the clock moving, where the runtime ends a parked wait
    /// itself, naming the causes due at that instant.
    ///
    /// # Panics
    ///
    /// As [`pump`](Self::pump) does.
    pub fn run_until_answered<T>(
        &mut self,
        answer: &mut AgentAnswer<T>,
        frame: Duration,
        limit: Duration,
    ) -> Option<Result<T, ErrorObject>> {
        let start = self.clock().elapsed();
        // The first turn drains the request at the instant it was made.
        let mut dt = Duration::ZERO;
        let mut turned_at = None;
        let mut turns_here = 0_u32;
        loop {
            let _ = self.pump(dt);
            if let Some(answered) = answer.try_take() {
                return Some(answered);
            }
            let now = flui_foundation::MonotonicClock::now(&self.clock);
            turns_here = if turned_at == Some(now) {
                turns_here.saturating_add(1)
            } else {
                1
            };
            turned_at = Some(now);
            let waited = self.clock().elapsed().saturating_sub(start);
            if turns_here > flui_runtime::settledness::TURNS_AT_ONE_INSTANT || waited >= limit {
                return None;
            }
            let unsettled = self.unsettled();
            let wake = unsettled.wake()?;
            let deadline_due = unsettled.next_deadline().is_some_and(|due| due <= now);
            dt = if wake > now {
                wake.saturating_duration_since(now)
            } else if deadline_due {
                Duration::ZERO
            } else {
                // Only causes the next turn serves re-armed at this instant:
                // the next frame is a display period later, or at the next
                // deadline if that comes sooner.
                unsettled
                    .next_deadline()
                    .map_or(frame, |due| frame.min(due.saturating_duration_since(now)))
            };
            dt = dt.min(limit.saturating_sub(waited));
        }
    }

    /// Stop [`pump`](Self::pump) running background work, so a test sees a
    /// widget while its work is still pending; the work waits until
    /// [`release_background`](Self::release_background) or
    /// [`run_background`](Self::run_background).
    pub fn hold_background(&mut self) {
        self.scripted.background.held.set(true);
    }

    /// Let [`pump`](Self::pump) run background work again, from its next
    /// call.
    pub fn release_background(&mut self) {
        self.scripted.background.held.set(false);
    }

    /// Run queued background work now, held or not, until none is runnable.
    /// Its answers settle at the next pump. Returns the jobs run plus the
    /// polls made.
    pub fn run_background(&mut self) -> usize {
        self.scripted.background.executors.run_until_idle()
    }

    /// Background jobs and futures queued and not yet finished. Counted by
    /// [`pump_until_settled`](Self::pump_until_settled): held work keeps a
    /// host unsettled.
    #[must_use]
    pub fn pending_background(&self) -> usize {
        self.scripted.background.pending()
    }

    /// Whether the host owes a turn, as a runner's owner is woken for one: a
    /// close a widget asked for, a close guard that signalled since the last
    /// settle (a hold released, a decision made), or a quit to ask the
    /// windows, to commit, or that waits on a window that is gone.
    fn turn_owed(&self) -> bool {
        !self.close_asks.borrow().is_empty()
            || self
                .ui_runtime
                .as_ref()
                .is_some_and(UiRuntime::owes_close_settle)
            || self.quit_owed
            || self.quit.is_asked()
            || self
                .quit
                .waiting()
                .iter()
                .any(|waited| !self.is_open(HeadlessWindowId(waited.presentation_id)))
    }

    /// Wait as a host's loop does, [`time_to_next_frame`](Self::time_to_next_frame),
    /// then pump. `None`, pumping nothing, when the loop would wait for input
    /// alone.
    ///
    /// # Panics
    ///
    /// As [`pump`](Self::pump) does.
    pub fn pump_next_frame(&mut self) -> Option<FrameOutcome> {
        let wait = self.time_to_next_frame()?;
        Some(self.pump(wait))
    }

    /// Pump frames of `frame` until the host's list
    /// ([`unsettled`](Self::unsettled)) names no cause, at most `max_frames`
    /// of them. Returns how many frames it took. A wait an agent parked is no
    /// cause: its deadline only wakes the loop.
    ///
    /// # Errors
    ///
    /// [`NotSettled`] when work remains after `max_frames` frames, such as a
    /// repeating animation.
    pub fn pump_until_settled(
        &mut self,
        frame: Duration,
        max_frames: usize,
    ) -> Result<usize, NotSettled> {
        for frames in 0..=max_frames {
            if self.unsettled().is_empty() {
                return Ok(frames);
            }
            if frames < max_frames {
                let _ = self.pump(frame);
            }
        }
        Err(NotSettled { frames: max_frames })
    }

    /// Apply `observation` of `presentation`'s window as a runner delivers
    /// one; a metrics change resizes the window's surface.
    pub(super) fn observe(
        &self,
        presentation: PresentationId,
        observation: flui_runtime::owner::WindowObservation,
    ) {
        // A real host hears nothing more of a window that closed.
        if !self.is_open(HeadlessWindowId(presentation)) {
            return;
        }
        if let flui_runtime::owner::WindowObservation::Metrics(metrics) = observation {
            self.window_of(HeadlessWindowId(presentation))
                .scale_factor
                .set(metrics.scale());
        }
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime().id(),
            presentation_id: presentation,
        };
        self.ui_runtime().enter(|ui_runtime| {
            ui_runtime.apply_window_observation(address, observation, &|_| {});
        });
        self.ask_owed_frames();
    }

    /// Install a scripted text-sizing provider on `window`: a captured source
    /// whose frontiers the host takes after each pump, as a runner hands them
    /// to the platform's text measurement, answered at once with each
    /// request's authored size until the test [holds](ScriptedTextSizing::hold)
    /// it. While held, the window's text preparation waits as it does for a
    /// measurement service that has not answered, until
    /// [`answer_text_sizing`](Self::answer_text_sizing).
    ///
    /// # Panics
    ///
    /// When the host is retired, already has a provider, or `window` is not
    /// open.
    pub fn script_text_sizing(&mut self, window: HeadlessWindowId) -> ScriptedTextSizing {
        assert!(
            self.text_sizing.is_none(),
            "a headless host takes one scripted text-sizing provider"
        );
        let (_policy, writer) = flui_painting::TextSizing::captured();
        let source = writer.source();
        assert!(
            self.ui_runtime()
                .install_captured_text_sizing_for(window.0, source),
            "the text-sizing provider is installed on an open window"
        );
        let (state, handle) = text_sizing::ScriptedTextSizingState::handle(writer);
        self.text_sizing = Some(state);
        handle
    }

    /// Answer every frontier the scripted text-sizing provider holds, as a
    /// measurement service's answer arriving, then ask each covered window for
    /// the frame it owes, as a runner's owner delivery does after the work
    /// item that settled them. How many frontiers the runtime accepted; none
    /// without a provider or a runtime.
    pub fn answer_text_sizing(&mut self) -> usize {
        let (Some(provider), Some(runtime)) = (&self.text_sizing, self.try_ui_runtime()) else {
            return 0;
        };
        let accepted = text_sizing::answer_held(provider, runtime);
        self.ask_owed_frames();
        accepted
    }

    /// Hand the frontiers the runtime asked for to the scripted text-sizing
    /// provider, if a test installed one.
    pub(super) fn serve_text_sizing(&self) {
        if let (Some(provider), Some(runtime)) = (&self.text_sizing, self.try_ui_runtime()) {
            text_sizing::serve(provider, runtime);
        }
    }

    /// Ask each window for the frame its presentation owes its surface, or
    /// withdraw the instant asked before, as a runner's owner delivery does
    /// after each work item (ADR-0389). A retired host asks nothing.
    pub(super) fn ask_owed_frames(&self) {
        let Some(runtime) = self.try_ui_runtime() else {
            return;
        };
        for request in runtime.owed_frame_requests() {
            request.deliver();
        }
    }

    /// Size the sink to the primary window's device size.
    pub(super) fn size_sink_to_primary(&mut self) {
        let physical = self.primary.window.physical_size_i32();
        self.sink.size = (
            u32::try_from(physical.width).expect("BUG: a window's device size is positive"),
            u32::try_from(physical.height).expect("BUG: a window's device size is positive"),
        );
    }
}

/// The protocol's name for the window of `presentation`.
fn wire_window(presentation: PresentationId) -> WindowId {
    WindowId::from_u64(presentation.as_u64())
        .expect("BUG: a presentation id packs a non-zero generation")
}
