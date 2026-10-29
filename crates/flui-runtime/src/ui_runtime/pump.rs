//! The UI runtime's frame transaction (ADR-0083 §1): the one entry a host drives a
//! frame through, and the background wake that runs no frame.

use super::{DrainReport, UiRuntime};
use crate::pump::{FrameClockSource, FrameOutcome};
use crate::sink::FrameSink;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

enum DeadlineWork {
    Observe,
    Poll,
}

/// Publishes a pump's frame timestamp for the frame's duration and restores it
/// on the way out, the unwinding way included. Rejected nested execution
/// restores its enclosing timestamp rather than clearing the outer frame.
struct FrameTimeGuard<'a> {
    ui_runtime: &'a UiRuntime,
    previous: Option<web_time::Instant>,
}

/// Native callback reentry cannot be expressed through the safe public
/// `pump(&mut self)` interface. This case reaches the same internal producer,
/// while observing public text-store grants and animation progress.
#[cfg(test)]
pub(super) fn rejected_nested_pump_preserves_outer_commits_and_animation_time() {
    use flui_animation::{Animation as _, AnimationController};
    use flui_platform::text_store::{
        InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
    };
    use flui_view::SignalWriteExt as _;
    use std::{cell::Cell, panic::AssertUnwindSafe, rc::Rc, sync::Arc, time::Duration};

    let input: Arc<dyn flui_platform::PlatformTextInput> =
        Arc::new(flui_native::FakeTextInput::new());
    let runtime = Rc::new(UiRuntime::for_test_with_text_input(Some(input)));
    runtime
        .enter(|runtime| runtime.attach_root_widget(&flui_widgets::SizedBox::square(10.0)))
        .expect("root attaches");
    let store = InMemoryTextStore::new("abc");
    let erased: Rc<dyn TextStore> = store.clone();
    let _client = runtime
        .text_input_handle()
        .attach(flui_interaction::TextInputClient::new(erased))
        .expect("text store attaches");
    let owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
    let controller = owner.controller();
    controller.forward().expect("animation starts");

    let run = |time| {
        runtime.pump_entered(
            &mut crate::pump::SampledClock(time),
            &mut crate::testing::ScriptedSink::always_presents(),
        )
    };
    let first = runtime.start + Duration::from_secs(1);
    let _ = run(first);

    let grants = Rc::new(Cell::new(0));
    let observed_grants = Rc::clone(&grants);
    let observed_store = store.clone();
    let signal = runtime
        .widgets()
        .with_build_owner(|owner| owner.signal_graph().signal(0u32));
    let commands = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_commands = Arc::clone(&commands);
    let weak_runtime = Rc::downgrade(&runtime);
    runtime
        .scheduler
        .add_persistent_frame_callback(Rc::new(move |_| {
            let runtime = weak_runtime.upgrade().expect("outer pump owns runtime");
            let commands = Arc::clone(&observed_commands);
            runtime
                .command_sender()
                .send_signal_write(signal.detach(), move |signal, graph| {
                    signal.set(graph, 1).expect("signal belongs to the runtime");
                    commands.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                })
                .expect("command admitted");
            let count = Rc::clone(&observed_grants);
            assert_eq!(
                observed_store.request_lock(
                    LockGrant::read_write(move |_| count.set(count.get() + 1)),
                    LockTiming::Async,
                ),
                Ok(LockOutcome::Deferred),
            );
            let nested = std::panic::catch_unwind(AssertUnwindSafe(|| {
                runtime.pump_entered(
                    &mut crate::pump::SampledClock(first + Duration::from_secs(5)),
                    &mut crate::testing::ScriptedSink::always_presents(),
                )
            }));
            let failure = nested.expect_err("recursive runtime execution is refused");
            assert!(
                failure.downcast_ref::<String>().is_some_and(|message| {
                    message.contains("must admit its frame transaction")
                }),
                "the runtime maps owner admission refusal to its invariant failure",
            );
            assert_eq!(observed_grants.get(), 0, "rejection runs no commit anchor");
            assert_eq!(
                observed_commands.load(std::sync::atomic::Ordering::Relaxed),
                0,
                "rejection leaves the accepted command pending"
            );
            assert_eq!(
                observed_store.request_lock(LockGrant::read(|_| {}), LockTiming::Sync),
                Err(TextStoreError::SyncLockUnavailable),
                "the outer frame still excludes native commits",
            );
        }));

    let _ = run(first + Duration::from_millis(500));
    assert_eq!(
        grants.get(),
        1,
        "the outer frame runs the queued grant once"
    );
    assert_eq!(
        store.request_lock(LockGrant::read(|_| {}), LockTiming::Sync),
        Ok(LockOutcome::Granted),
        "the completed outer frame reopens commits",
    );
    let value = controller.value();
    assert!(
        (value - 0.5).abs() < 1e-9,
        "outer animation time survives: {value}"
    );
    assert_eq!(
        runtime.drain_commands().invoked,
        1,
        "the next boundary receives the command"
    );
    assert_eq!(
        commands.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "rejected execution neither invokes nor loses the command"
    );
}

impl<'a> FrameTimeGuard<'a> {
    fn publish(ui_runtime: &'a UiRuntime, frame_time: web_time::Instant) -> Self {
        let previous = ui_runtime.frame_time.replace(Some(frame_time));
        Self {
            ui_runtime,
            previous,
        }
    }
}

impl Drop for FrameTimeGuard<'_> {
    fn drop(&mut self) {
        self.ui_runtime.frame_time.set(self.previous);
    }
}

impl UiRuntime {
    /// The frame transaction (ADR-0083 §1), in this order:
    ///
    /// 1. apply commands: drain the owner inbox at the Idle boundary. A host
    ///    gate has usually drained it already this wake
    ///    ([`Self::drain_owner_inbox`]), so this catches only what a worker
    ///    sent in between; the redraw request such a command raises stays
    ///    set, and the host's next gate reads it (one extra frame at most);
    /// 2. begin frame: transient callbacks, microtasks and the one mid-frame
    ///    async poll, at the timestamp `clock` returns;
    /// 3. draw frame: persistent callbacks, then this UI runtime's pipeline and
    ///    the submit through `sink` (the crate-private `render_frame`);
    /// 4. end frame: the shared post-frame queue and this UI runtime's owner-local
    ///    post-frame lane, in one total order;
    /// 5. commit anchor: steps 2–4 are every presentation's text-store
    ///    transaction, its commit gate shut for their whole duration, so an
    ///    input method's lock asked for inside them is queued; the queued
    ///    grants run here, with the scheduler back in `Idle` (ADR-0027 §3).
    ///
    /// The mutable interface prevents ordinary overlapping host calls. Owner
    /// admission also refuses callback reentry before timestamp publication,
    /// command application or geometry service. The pump enters the UI runtime
    /// itself for the whole transaction.
    ///
    /// It is also the only way a host outside this crate draws a frame:
    ///
    /// ```no_run
    /// use flui_runtime::pump::SampledClock;
    /// use flui_runtime::sink::FrameSink;
    /// use flui_runtime::ui_runtime::UiRuntime;
    ///
    /// fn frame(ui_runtime: &mut UiRuntime, sink: &mut dyn FrameSink) -> bool {
    ///     ui_runtime.pump(&mut SampledClock(web_time::Instant::now()), sink).presented()
    /// }
    /// ```
    ///
    /// The draw step on its own, without begin and end frame, is not
    /// reachable. `trybuild_ui::ui_tests` pins the private-method diagnostic
    /// alongside a valid host pump caller:
    ///
    /// ```compile_fail
    /// use flui_runtime::sink::FrameSink;
    /// use flui_runtime::ui_runtime::UiRuntime;
    ///
    /// fn frame(ui_runtime: &mut UiRuntime, sink: &mut dyn FrameSink) -> bool {
    ///     ui_runtime.render_frame(sink)
    /// }
    /// ```
    ///
    /// The clock is read once. That instant is the scheduler's frame
    /// timestamp and the time the UI runtime's `Vsync` controllers tick at
    /// (`raw_frame_time` reads it for the frame's duration), projected through
    /// each presentation's motion clock. Scheduler tickers are retired;
    /// complete owner execution is described by ADR-0175/0182.
    /// Whether a wake becomes a frame at all is the host's decision (its wake
    /// gate, ADR-0058), not this method's.
    pub fn pump(
        &mut self,
        clock: &mut dyn FrameClockSource,
        sink: &mut dyn FrameSink,
    ) -> FrameOutcome {
        self.pump_entered(clock, sink)
    }

    fn pump_entered(
        &self,
        clock: &mut dyn FrameClockSource,
        sink: &mut dyn FrameSink,
    ) -> FrameOutcome {
        // Records a tap admitted reentrantly wait for this owner turn.
        self.begin_recorded_input();
        self.deliver_input_records();
        // An anchor an earlier turn owes runs before this frame builds. The
        // frame runs even when it fails: that failure came first, and is
        // raised once the frame is done.
        let paid = catch_unwind(AssertUnwindSafe(|| {
            self.enter(UiRuntime::pay_owed_anchor);
        }))
        .err();
        let sampled = clock.frame_time();
        let pending = self.pending_frame_request();
        let request = pending.map(|(request, _)| request);
        let now = self.frame_time_for(request, sampled);
        self.last_frame_time.set(Some(now));
        // Idle work runs until the instant the platform names for this frame's
        // present, else until the next refresh the presentation's newest
        // presentation report places, else until the host's deadline.
        let deadline = pending
            .and_then(|(request, presentation)| {
                request
                    .target_present
                    .and_then(|shown| self.instant_of(shown))
                    .or_else(|| presentation.clock().next_presentation_after(now))
            })
            .map_or_else(
                || clock.idle_deadline(now),
                flui_scheduler::IdleDeadline::from,
            );
        let mut frame_time = None;
        let mut geometry_turn = None;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.enter(|ui_runtime| {
                // Begin, draw and end frame run as the ui_runtime's text-store
                // transaction, with the commit anchor after it (ADR-0027 §3).
                let presented = ui_runtime.drive_frame(
                    now,
                    deadline,
                    || {
                        frame_time = Some(FrameTimeGuard::publish(self, now));
                        // Before the frame begins: it copies the budget's target.
                        ui_runtime.publish_frame_budget();
                        geometry_turn = Some(ui_runtime.begin_geometry_turn());
                        let report = ui_runtime.drain_commands();
                        if report != DrainReport::default() {
                            tracing::trace!(?report, "owner inbox drained at pump start");
                        }
                        ui_runtime.service_gesture_geometry(
                            geometry_turn
                                .as_ref()
                                .expect("BUG: admitted preparation installed its turn"),
                        );
                    },
                    || ui_runtime.render_frame(sink),
                );
                FrameOutcome::new(presented)
            })
        }));
        let admitted = frame_time.is_some();
        drop(geometry_turn);
        drop(frame_time);
        let outcome = match paid {
            None => outcome,
            Some(failure) => {
                let mut first = Some(failure);
                crate::lifecycle_state::preserve_first_lifecycle_panic(
                    &mut first,
                    outcome.err(),
                    "frame after the owed anchor",
                );
                Err(first.expect("BUG: the owed anchor's failure stays first"))
            }
        };
        let outcome = self.wake_for_owed_anchor(outcome);
        self.clear_frame_requests();
        // The verdicts the frame made (deadlines) follow its frame row.
        self.deliver_input_records();
        if !admitted {
            return outcome.unwrap_or_else(|failure| resume_unwind(failure));
        }
        self.finish_owner_activity(outcome, DeadlineWork::Observe)
    }

    /// A wake that runs no frame: clear the scheduler's frame latch, drain
    /// commands and service geometry, then poll ready async tasks once.
    /// Due input deadlines run after that transaction even while visual frame
    /// gates are closed. No begin frame, animation, build, layout, paint or
    /// present. Hosts call it for a wake whose gate found frames disabled
    /// (the app is hidden, paused or detached), and iOS for every owner turn,
    /// which commits commands and polls, frames enabled or not.
    ///
    /// The order is load-bearing. No begin frame consumes the old demand
    /// here, so the background operation must do it before preparation and
    /// polling. Polling first would let a future that schedules a
    /// frame find the latch still set, fire no wake, and starve until
    /// unrelated input arrives. The owner performs the complete background
    /// operation. Command application, geometry service and redraw acknowledgment
    /// run after admission and demand consumption, before polling. Preparation
    /// failure preserves ready tasks and retries their wake delivery.
    pub fn pump_background(&mut self) {
        self.begin_recorded_input();
        self.deliver_input_records();
        let mut turn = None;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.enter(|ui_runtime| {
                ui_runtime
                    .owner_frame
                    .pump_background(|| {
                        turn = Some(ui_runtime.begin_geometry_turn());
                        ui_runtime.drain_owner_inbox();
                    })
                    .expect("BUG: the runtime's live frame owner must admit its background turn");
            });
        }));
        let admitted = turn.is_some();
        drop(turn);
        if !admitted {
            return outcome.unwrap_or_else(|failure| resume_unwind(failure));
        }
        self.finish_owner_activity(outcome, DeadlineWork::Poll);
    }

    /// Commit the owner inbox at the Idle boundary and report whether the
    /// drain asked for a redraw.
    ///
    /// A host's frame wake calls this once per wake, inside the UI runtime's
    /// entry and before its wake gate, so a command-driven redraw request is
    /// seen by the very wake it produced — and before any early return the
    /// wake takes. Draining on every wake is what keeps the bounded inbox
    /// from filling; the coalesced redraw request is consumed here.
    pub fn drain_owner_inbox(&self) -> bool {
        let turn = self.begin_geometry_turn();
        // The owed anchor first; the inbox drains even if it fails, so the
        // bounded inbox never fills behind a failing grant.
        let mut first = catch_unwind(AssertUnwindSafe(|| self.pay_owed_anchor())).err();
        let drained = catch_unwind(AssertUnwindSafe(|| {
            let report = self.drain_commands();
            if report != DrainReport::default() {
                tracing::trace!(?report, "owner inbox drained");
            }
            self.service_gesture_geometry(&turn);
            self.take_redraw_request()
        }));
        drop(turn);
        let outcome = match drained {
            Ok(redraw) => first.map_or(Ok(redraw), Err),
            Err(failure) => {
                crate::lifecycle_state::preserve_first_lifecycle_panic(
                    &mut first,
                    Some(failure),
                    "owner inbox drain",
                );
                Err(first.expect("BUG: a failed drain keeps a failure"))
            }
        };
        let outcome = self.wake_for_owed_anchor(outcome);
        self.finish_owner_activity(outcome, DeadlineWork::Observe)
    }

    /// Ask for a wake while the commit anchor is still owed after this turn
    /// (a frame unwound, or an anchor failed), so the next owner turn comes
    /// and pays it. A wake that fails is contained behind the turn's own
    /// failure and leaves the debt in place.
    fn wake_for_owed_anchor<T>(&self, outcome: std::thread::Result<T>) -> std::thread::Result<T> {
        if !self.owes_anchor() {
            return outcome;
        }
        let woke = catch_unwind(AssertUnwindSafe(|| self.request_redraw())).err();
        match outcome {
            Ok(value) => woke.map_or(Ok(value), Err),
            Err(failure) => {
                let mut first = Some(failure);
                crate::lifecycle_state::preserve_first_lifecycle_panic(
                    &mut first,
                    woke,
                    "owed anchor wake",
                );
                Err(first.expect("BUG: the turn's own failure stays first"))
            }
        }
    }

    /// Commands, deferred grants and async callbacks may replace a deadline
    /// source. Refresh after their operation, preserving its original unwind.
    fn finish_owner_activity<T>(
        &self,
        outcome: std::thread::Result<T>,
        deadlines: DeadlineWork,
    ) -> T {
        let (value, mut first) = match outcome {
            Ok(value) => (Some(value), None),
            Err(failure) => (None, Some(failure)),
        };
        let state = if first.is_some() {
            crate::owner::RecoveryState::PreservingFailure
        } else {
            crate::owner::RecoveryState::Healthy
        };
        let outer_turn_finished = self.geometry_turn.borrow().is_none();
        let refresh = catch_unwind(AssertUnwindSafe(|| {
            match deadlines {
                // An inbox drain can precede its enclosing frame's sampling.
                // Discovery waits for the completed owner transaction, so a
                // failed retained attempt cannot be replaced in that same pump.
                DeadlineWork::Observe if outer_turn_finished => {
                    self.refresh_deadline_observations(state);
                }
                DeadlineWork::Observe => {}
                // A native background wake runs no visual sampling. Its input
                // deadlines must still progress while frame gates are closed.
                DeadlineWork::Poll => self.sample_background_deadlines(state),
            }
        }))
        .err();
        crate::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first,
            refresh,
            "owner scheduling observation",
        );
        // An inbox drain can run inside a larger pump/background transaction.
        // Its reports wait for that enclosing turn as well as any owner lease.
        if outer_turn_finished {
            let delivery = catch_unwind(AssertUnwindSafe(|| {
                self.deliver_sampling_reports_if_unleased();
            }))
            .err();
            crate::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                delivery,
                "owner scheduling reports",
            );
        }
        if let Some(failure) = first {
            resume_unwind(failure);
        }
        value.expect("BUG: a successful owner operation retains its result")
    }
}

impl UiRuntime {
    /// The request of the newest frame delivered since the last pump, if any host delivered one,
    /// with the presentation it was delivered to.
    fn pending_frame_request(
        &self,
    ) -> Option<(
        flui_platform::FrameRequest,
        &crate::presentation::PresentationState,
    )> {
        self.presentations
            .iter()
            .filter_map(|presentation| {
                presentation
                    .frame_request
                    .get()
                    .map(|request| (request, presentation))
            })
            .max_by_key(|(request, _)| request.vsync)
    }

    /// Forget requests this pump did not render, so a skipped frame's vsync never dates a later
    /// wake.
    fn clear_frame_requests(&self) {
        for presentation in self.presentations.iter() {
            presentation.frame_request.set(None);
        }
    }

    /// The instant on this runtime's clock that `time` names, or `None` past the last one the
    /// platform represents.
    pub(super) fn instant_of(&self, time: flui_platform::EventTime) -> Option<web_time::Instant> {
        flui_foundation::MonotonicClock::epoch(&self.clock)
            .checked_add(std::time::Duration::from_nanos(time.as_nanos()))
    }

    /// The time this frame runs at (ADR-0257): the delivered vsync, held between the previous
    /// frame's time (the clock never runs backwards) and `sampled`, the host's own reading when it
    /// pumped (a platform timestamp ahead of that is a defect, not a time). Without a vsync, or
    /// one off the clock's range, the sampled reading.
    fn frame_time_for(
        &self,
        request: Option<flui_platform::FrameRequest>,
        sampled: web_time::Instant,
    ) -> web_time::Instant {
        let Some(vsync) = request.and_then(|request| self.instant_of(request.vsync)) else {
            return sampled;
        };
        let floor = self
            .last_frame_time
            .get()
            .map_or(vsync, |last| vsync.max(last));
        floor.min(sampled)
    }
}
