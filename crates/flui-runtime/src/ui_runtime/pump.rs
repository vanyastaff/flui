//! The UI runtime's frame transaction (ADR-0083 §1): the one entry a host drives a
//! frame through, and the background wake that runs no frame.

use super::{DrainReport, UiRuntime};
use crate::pump::{FrameClockSource, FrameOutcome};
use crate::sink::FrameSink;

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
    use flui_platform_api::text_store::{
        InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
    };
    use std::{cell::Cell, panic::AssertUnwindSafe, rc::Rc, sync::Arc, time::Duration};

    let input: Arc<dyn flui_platform_api::PlatformTextInput> =
        Arc::new(flui_platform::FakeTextInput::new());
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
        let turn = runtime.begin_geometry_turn();
        runtime.pump_entered(
            &mut crate::pump::SampledClock(time),
            &mut crate::testing::ScriptedSink::always_presents(),
            &turn,
        )
    };
    let first = runtime.start + Duration::from_secs(1);
    let _ = run(first);

    let grants = Rc::new(Cell::new(0));
    let observed_grants = Rc::clone(&grants);
    let observed_store = store.clone();
    let weak_runtime = Rc::downgrade(&runtime);
    runtime
        .scheduler
        .add_persistent_frame_callback(Rc::new(move |_| {
            let runtime = weak_runtime.upgrade().expect("outer pump owns runtime");
            let count = Rc::clone(&observed_grants);
            assert_eq!(
                observed_store.request_lock(
                    LockGrant::read_write(move |_| count.set(count.get() + 1)),
                    LockTiming::Async,
                ),
                Ok(LockOutcome::Deferred),
            );
            let nested = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let turn = runtime.begin_geometry_turn();
                runtime.pump_entered(
                    &mut crate::pump::SampledClock(first + Duration::from_secs(5)),
                    &mut crate::testing::ScriptedSink::always_presents(),
                    &turn,
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
    /// `&mut self` is the point: the compiler rules out starting a frame
    /// while another frame on this UI runtime is running. The pump enters the
    /// UI runtime itself for the whole transaction.
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
    /// (`raw_frame_time` reads it for the frame's duration); a scheduler `Ticker`
    /// still measures elapsed time on the wall clock (`flui-scheduler`'s
    /// `ARCHITECTURE.md`).
    /// Whether a wake becomes a frame at all is the host's decision (its wake
    /// gate, ADR-0058), not this method's.
    pub fn pump(
        &mut self,
        clock: &mut dyn FrameClockSource,
        sink: &mut dyn FrameSink,
    ) -> FrameOutcome {
        let turn = self.begin_geometry_turn();
        self.pump_entered(clock, sink, &turn)
    }

    fn pump_entered(
        &self,
        clock: &mut dyn FrameClockSource,
        sink: &mut dyn FrameSink,
        turn: &super::preferences::GeometryTurn,
    ) -> FrameOutcome {
        let now = clock.frame_time();
        let deadline = clock.idle_deadline(now);
        let _frame_time = FrameTimeGuard::publish(self, now);
        self.enter(|ui_runtime| {
            let report = ui_runtime.drain_commands();
            if report != DrainReport::default() {
                tracing::trace!(?report, "owner inbox drained at pump start");
            }
            ui_runtime.service_gesture_geometry(turn);
            // Begin, draw and end frame run as the ui_runtime's text-store
            // transaction, with the commit anchor after it (ADR-0027 §3).
            let presented = ui_runtime.drive_frame(now, deadline, || ui_runtime.render_frame(sink));
            FrameOutcome::new(presented)
        })
    }

    /// A wake that runs no frame: clear the scheduler's frame latch, then
    /// poll the UI runtime's ready async tasks once. No begin frame, no tickers, no pipeline,
    /// no present. Hosts call it for a wake whose gate found frames disabled
    /// (the app is hidden, paused or detached), and iOS for every owner turn,
    /// which only commits commands and polls, frames enabled or not.
    ///
    /// The order is load-bearing. Only a begin frame clears the latch, and
    /// none runs here; polling first would let a future that schedules a
    /// frame find the latch still set, fire no wake, and starve until
    /// unrelated input arrives. The owner performs the complete background
    /// operation, servicing geometry between demand consumption and polling.
    pub fn pump_background(&mut self) {
        let turn = self.begin_geometry_turn();
        self.enter(|ui_runtime| {
            ui_runtime
                .owner_frame
                .pump_background(|| ui_runtime.service_gesture_geometry(&turn))
                .expect("BUG: the runtime's live frame owner must admit its background turn");
        });
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
        let report = self.drain_commands();
        if report != DrainReport::default() {
            tracing::trace!(?report, "owner inbox drained");
        }
        self.service_gesture_geometry(&turn);
        self.take_redraw_request()
    }
}
