//! How a [`HeadlessHost`] decides it has settled: waits, background work and text
//! sizing.

#[cfg(doc)]
use super::HeadlessWindow;

use std::time::Duration;

use flui_foundation::PresentationId;

use flui_runtime::pump::FrameOutcome;

use flui_runtime::ui_runtime::UiRuntime;

use super::errors::NotSettled;
use super::text_sizing::{self, ScriptedTextSizing};
use super::{HeadlessHost, HeadlessWindowId};

impl HeadlessHost {
    /// How long a host's loop waits before its next frame: none while a frame
    /// is asked for or work is pending, unless a dropped frame waits for its
    /// retry; otherwise until the earliest deadline the UI runtime armed, such
    /// as a gesture's or a dropped frame's retry, the instant a window asked
    /// for a frame at (the frame a covered window owes its surface), or the
    /// next change the host's scenario scheduled. `None` when the loop would
    /// wait for input alone.
    ///
    /// Not a pure read: it first asks each covered window for the frame it
    /// owes, as a runner's owner delivery does after the work item that left
    /// it owed, which runs the windows' `request_frame_at` and records the
    /// request ([`HeadlessWindow::timed_frame_requests`]). Asking again with
    /// nothing changed asks nothing.
    #[must_use]
    pub fn time_to_next_frame(&self) -> Option<Duration> {
        let Some(runtime) = &self.ui_runtime else {
            // A retired host frames nothing: its loop wakes for the
            // background lanes and the scenario alone.
            if self.background_runs_next_pump() {
                return Some(Duration::ZERO);
            }
            return self.scenario_wait();
        };
        // What changed since the last turn (a signal written between pumps)
        // may leave a covered window owing a frame: its window is asked first,
        // as a runner's owner delivery asks at the end of the work item that
        // changed it, and the wait below counts the request.
        self.ask_owed_frames();
        let now = flui_foundation::MonotonicClock::now(&self.clock);
        // A dropped frame waiting for its retry runs nothing sooner, though a
        // gesture deadline before it still wakes the loop.
        let retrying = runtime
            .failure_retry_deadline()
            .is_some_and(|retry| retry > now);
        // A runtime whose frames are off (no window seen) has no frame to run
        // now: its pump is a background turn, which its deadlines, background
        // work and observations still ask for. The frame a covered window owes
        // is served only through its window's timed request, as on a native
        // host, never as work pending here.
        let frame_owed = runtime.scheduler().frames_enabled()
            && (runtime.needs_redraw()
                || runtime.scheduler().is_frame_scheduled()
                || runtime.has_pending_work());
        if !retrying
            && (frame_owed
                || self.background_runs_next_pump()
                || self.observations_pending()
                || self.turn_owed())
        {
            return Some(Duration::ZERO);
        }
        let runtime_wait = runtime
            .next_wake()
            .map(|wake| wake.saturating_duration_since(now));
        // A change the scenario has scheduled is a deadline like any other, and
        // so is a window's timed frame request, which the host serves.
        [runtime_wait, self.scenario_wait(), self.timed_frame_wait()]
            .into_iter()
            .flatten()
            .min()
    }

    /// How long until the scenario's next scheduled change, if one is.
    fn scenario_wait(&self) -> Option<Duration> {
        let scenario = self.scripted.clock.scenario();
        scenario
            .next_due()
            .map(|due| due.saturating_sub(scenario.now()))
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

    /// Whether a window changed since the host last told the UI runtime: the next pump
    /// delivers the change, so the host is not settled.
    fn observations_pending(&self) -> bool {
        self.presentation_ids().into_iter().any(|presentation| {
            self.window_of(HeadlessWindowId(presentation))
                .chrome
                .borrow()
                .unobserved
        })
    }

    /// Whether the next pump runs background work: a parked future waits
    /// for its wake, not for a frame.
    fn background_runs_next_pump(&self) -> bool {
        !self.scripted.background.held.get() && self.scripted.background.executors.runnable() > 0
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

    /// Pump frames of `frame` until nothing is left to do — no redraw or
    /// frame asked for (a running animation asks for each next one), no
    /// pending work, no armed deadline, no unserved timed frame request of a
    /// window, no change the scenario scheduled — at most `max_frames` of
    /// them. Returns how many frames it took. Before each check it asks each
    /// covered window for the frame it owes, as
    /// [`time_to_next_frame`](Self::time_to_next_frame) does.
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
            // A covered window owing a frame is asked before the host decides
            // it has nothing to do, as `time_to_next_frame` asks.
            self.ask_owed_frames();
            // A retired host owes only its background work and the scenario.
            // A runtime whose frames are off owes no frame, only its deadlines
            // and the timed requests of its windows.
            let runtime_idle = self.ui_runtime.as_ref().is_none_or(|runtime| {
                (!runtime.scheduler().frames_enabled()
                    || (!runtime.needs_redraw()
                        && !runtime.scheduler().is_frame_scheduled()
                        && !runtime.has_pending_work()))
                    && runtime.next_wake().is_none()
            });
            if runtime_idle
                && self.timed_frame_wait().is_none()
                && self.pending_background() == 0
                && !self.observations_pending()
                && !self.turn_owed()
                && self.scenario_wait().is_none()
            {
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

    /// How long until the earliest timed frame request of an open window the
    /// host has not served, if any.
    fn timed_frame_wait(&self) -> Option<Duration> {
        let now = self.event_now();
        self.open
            .iter()
            .filter_map(|open| open.window.pending_timed_frame())
            .min()
            .map(|at| at.saturating_duration_since(now))
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
