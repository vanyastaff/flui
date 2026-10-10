// ============================================================================
// GPU device-loss recovery (App.1 device-recovery wake)
// ============================================================================
//
// The sync device-rebuild seam plus the shared retry/backoff logic the
// desktop, Android and iOS frame drivers use around `UiRuntime::pump`. Web's own recovery (`bootstrap_web`)
// stays un-unified: its `recover()` is driven through
// `wasm_bindgen_futures::spawn_local`, not a synchronous call this trait
// could wrap — see that call site's own comment for why.

/// The sync device-rebuild seam the frame driver needs from its renderer.
///
/// [`flui_engine::RasterBackend`] deliberately excludes recovery — it is
/// async and window-handle-specific (see `flui-engine/src/raster.rs`'s
/// trait doc) — so the runners narrow the concrete renderer to this seam
/// instead of widening the public trait. The production impl wraps
/// `Renderer::recover` in `pollster::block_on`; test fakes script the
/// outcome. `is_device_lost` is NOT duplicated here — it already lives on
/// `RasterBackend`, and every consumer bounds on both traits.
#[cfg(not(target_arch = "wasm32"))]
pub(super) trait DeviceRecovery {
    /// Attempt to rebuild the lost device synchronously on the runner
    /// thread.
    fn try_recover_device(&mut self) -> Result<(), flui_engine::EngineError>;
}

#[cfg(not(target_arch = "wasm32"))]
impl DeviceRecovery for flui_engine::Renderer {
    fn try_recover_device(&mut self) -> Result<(), flui_engine::EngineError> {
        // `pollster` is already a dep and safe to use here — the
        // desktop/Android runners own synchronous platform callbacks, not
        // an async executor.
        pollster::block_on(self.recover())
    }
}

/// Alias for the shared [`super::retry_backoff::RetryBackoff`], carrying the
/// subject label device-loss recovery's log lines use. The policy lives in one
/// place (`retry_backoff.rs`); this alias is what keeps every call site in this
/// module reading `DeviceRecoveryBackoff` while the implementation is shared
/// with surface-recreation retry.
#[cfg(not(target_arch = "wasm32"))]
pub(super) type DeviceRecoveryBackoff = super::retry_backoff::RetryBackoff;

/// The label this recovery's backoff logs under.
#[cfg(not(target_arch = "wasm32"))]
pub(super) const DEVICE_RECOVERY_LABEL: &str = "GPU device recovery";

/// Construct this module's backoff: the shared [`super::retry_backoff::RetryBackoff`]
/// with device recovery's own label already bound. A free function rather than
/// an inherent `new` because the type is an alias for the shared one, and this
/// is the one place the label is chosen.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn new_device_recovery_backoff() -> DeviceRecoveryBackoff {
    DeviceRecoveryBackoff::new(DEVICE_RECOVERY_LABEL)
}

/// Outcome of one call to [`attempt_device_recovery`].
#[cfg(not(target_arch = "wasm32"))]
enum RecoveryAttempt {
    /// The backoff's armed deadline had not yet elapsed — no attempt was
    /// made. Carries that SAME deadline (not a freshly computed one).
    Deferred(web_time::Instant),
    /// A fresh attempt failed; the caller records its backoff outside the lane.
    Failed(flui_engine::EngineError),
    /// A fresh attempt was made and succeeded.
    Recovered,
}

/// Attempt one synchronous device rebuild — but only if `backoff`'s deadline
/// has elapsed; otherwise returns immediately without touching the renderer
/// or the backoff's counters. See [`DeviceRecoveryBackoff`]'s own doc for
/// why this is a deadline CHECK, never a sleep: skipping is the only
/// non-blocking way to pace an attempt that can cost a full GPU stack
/// rebuild.
#[cfg(not(target_arch = "wasm32"))]
fn attempt_device_recovery<R: DeviceRecovery>(
    renderer: &mut R,
    backoff: &DeviceRecoveryBackoff,
    now: web_time::Instant,
) -> RecoveryAttempt {
    if let Some(deadline) = backoff.next_attempt_at()
        && now < deadline
    {
        return RecoveryAttempt::Deferred(deadline);
    }
    match renderer.try_recover_device() {
        Ok(()) => RecoveryAttempt::Recovered,
        Err(error) => RecoveryAttempt::Failed(error),
    }
}

/// Outcome of driving one frame through [`pump_with_device_recovery`].
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct FrameRecoveryOutcome {
    /// Whether the frame reached `present()` — same meaning as
    /// [`FrameOutcome::presented`](flui_runtime::pump::FrameOutcome::presented).
    // Read only by the desktop runner's fallback-pacing arm; the mobile and
    // web runners pace from their own frame sources and do not consult it.
    #[cfg_attr(
        all(
            not(all(test, not(any(target_os = "android", target_os = "ios")))),
            any(target_os = "android", target_os = "ios", target_arch = "wasm32")
        ),
        expect(
            dead_code,
            reason = "consumed only by the desktop runner's fallback pacing"
        )
    )]
    pub(super) presented: bool,
    /// Set exactly when a NEW recovery attempt failed this call — never on
    /// a merely-deferred attempt (the backoff deadline had not elapsed) and
    /// never on success or lane contention. Recovery failures and an unchecked
    /// post-frame device state arm their retry wake after the pump's tail.
    ///
    /// Read only by this module's own tests today: production callers
    /// (`bootstrap_desktop`/`bootstrap_android`) consult the persistent
    /// `DeviceRecoveryBackoff` directly (via its own `next_attempt_at`) for
    /// the wake-deadline hook rather than this per-call snapshot, so this
    /// field carries no separate production obligation of its own — kept
    /// on the struct anyway because it is what makes the wake-on-failure-
    /// only contract independently checkable per call.
    // Dependency builds can cap dead_code while still checking expectations.
    #[cfg_attr(
        not(all(test, not(any(target_os = "android", target_os = "ios")))),
        allow(dead_code, reason = "read by this module's own tests only")
    )]
    pub(super) just_failed: bool,
    /// The earliest instant the next recovery attempt is allowed, if the
    /// device is still (or newly) lost. `None` once healthy.
    ///
    /// Read only by this module's own tests today, for the same reason as
    /// [`Self::just_failed`] — production callers consult
    /// `DeviceRecoveryBackoff::next_attempt_at` directly instead.
    #[cfg_attr(
        not(all(test, not(any(target_os = "android", target_os = "ios")))),
        allow(dead_code, reason = "read by this module's own tests only")
    )]
    pub(super) next_attempt_at: Option<web_time::Instant>,
}

/// Drive one frame through `ui_runtime` against `lane`'s raster mailbox,
/// rebuilding a lost device around it.
///
/// A device already lost at frame start gets a backoff-gated recovery
/// attempt here, BEFORE
/// the UI runtime's pump through the lane (whose draw phase is
/// `UiRuntime::render_frame`) — but that frame drive runs regardless of
/// whether that attempt happened or what it returned. Skipping it on a still-lost device was the
/// original bug this function fixes: `render_frame` is the only
/// caller of `drain_deferred_arena_resolutions`, `flush_pending_moves`,
/// `draw_frame_entered`, and `mouse_tracker().update_all_devices()`, all of
/// which must keep advancing while the device is down (gesture-arena
/// deadlines, coalesced pointer moves, and every Vsync ticker included) —
/// and `Renderer::acquire_surface_texture` already bails out on the same
/// `device_lost` flag before touching the GPU, so running it costs nothing
/// extra on a still-dead device.
///
/// A device lost or re-lost MID-frame (the wgpu device-lost callback firing
/// while `render_scene` ran) gets its own attempt AFTER. The post-frame
/// check is gated on `next_attempt_at.is_none()`, not on "was the device
/// lost before this call": a pre-frame attempt that SUCCEEDS leaves
/// `next_attempt_at` at `None`, so if the SAME call's `render_frame`
/// then loses the device again, the post-frame check still fires — a
/// `was_lost_pre_frame`-style gate would silently miss exactly that
/// recovered-then-re-lost-mid-frame case, since it only ever asks about the
/// PRE-frame state. Either check finding a non-`None` `next_attempt_at`
/// means a decision (failed or deferred) was already recorded THIS call, so
/// the other one skips — attempting twice in one wake would silently double
/// the effective retry rate the backoff exists to bound.
///
/// A SUCCESSFUL PRE-frame attempt calls [`crate::app::ui_runtime::UiRuntime::
/// mark_primary_needs_full_repaint`] — never [`crate::app::ui_runtime::UiRuntime::
/// wake_frame`], and never bare [`crate::app::ui_runtime::UiRuntime::request_redraw`]
/// either. Neither of those alone is enough: `wake_frame` does not open
/// `render_frame`'s OWN per-presentation dirty gate
/// (`presentation.take_redraw_pending()`, `ui_runtime/`'s
/// `draw_frame_entered`) at all, and `request_redraw` alone opens that gate
/// but still leaves `PipelineOwner`'s OWN independent dirty tracking
/// untouched — confirmed by a probe, not assumed: with `request_redraw`
/// alone, `draw_frame_entered`'s segment gate correctly read `Produce`, and
/// the pipeline STILL emitted `Idle` because nothing told it there was work
/// to redo. `mark_primary_needs_full_repaint` does both: it is what a
/// recovered device — whose own backing store was invalidated by the loss;
/// `Renderer::recover` already primes a full repaint via `mark_full_repaint`
/// for whatever scene reaches it next — needs to actually get a fresh scene
/// submitted, rather than staying visually blank until something unrelated
/// happens to dirty the tree. No platform poke alongside it (unlike a
/// failure): this call's OWN `render_frame` below runs
/// synchronously right after, in the very same wake, so there is nothing
/// external left to wake. The POST-frame (mid-frame-loss) success arm calls
/// neither: that frame already had its own chance to present before the
/// loss was even noticed, so there is no known-blank backing store to force
/// a fresh submit for.
///
/// The two checks bracket the whole [`UiRuntime::pump`](crate::app::ui_runtime::UiRuntime::pump),
/// not just its pipeline: the pre-frame attempt runs before begin frame and
/// the post-frame one after the post-frame callbacks. Neither touches the
/// tree except `mark_primary_needs_full_repaint`, which still lands before
/// the pipeline that repaints. `now` is also the pump's frame timestamp.
///
/// Only recovery and scene submission borrow the raster lane. Owner callbacks
/// run without that guard. A busy initial lane returns `None`, retaining a wake
/// without consuming frame work; contention during submission returns `Retry`.
/// A busy post-frame check also retains a wake without claiming device loss.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn pump_with_device_recovery<B>(
    ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
    lane: &parking_lot::Mutex<crate::app::raster_lane::RasterLane<B>>,
    backoff: &DeviceRecoveryBackoff,
    now: web_time::Instant,
) -> Option<FrameRecoveryOutcome>
where
    B: flui_engine::RasterBackend + DeviceRecovery,
{
    use flui_runtime::pump::SampledClock;

    let mut just_failed = false;
    let mut next_attempt_at = None;
    let Some((mut sink, mut guard)) = crate::app::raster_lane::RasterFrame::try_new(lane) else {
        ui_runtime.wake_frame();
        return None;
    };

    let pre_attempt = if guard.is_device_lost() {
        let attempt =
            guard.with_backend(|renderer| attempt_device_recovery(renderer, backoff, now));
        if matches!(attempt, RecoveryAttempt::Recovered) {
            guard.note_surface_recreated();
        }
        Some(attempt)
    } else {
        None
    };
    drop(guard);

    if let Some(attempt) = pre_attempt {
        match attempt {
            RecoveryAttempt::Recovered => {
                // Recovery rebuilt the surface, so the lane re-mints its
                // `SurfaceGeneration` through the mailbox's one counter
                // (ADR-0045 decision 4 names recovery's surface recreation
                // as a mint site) — without this, the recovered lane's
                // next frame would still stamp the pre-loss generation.
                backoff.record_success();
                tracing::warn!("GPU device lost — recovered successfully");
                // Pre-frame only: nothing has presented since the device
                // died, so the recovered backing store needs a genuinely
                // fresh submit — see `UiRuntime::mark_primary_needs_full_
                // repaint`'s own doc for why `request_redraw` alone does
                // not get one.
                ui_runtime.mark_primary_needs_full_repaint();
            }
            RecoveryAttempt::Failed(error) => {
                just_failed = true;
                next_attempt_at = Some(backoff.record_failure(&error, now));
            }
            RecoveryAttempt::Deferred(deadline) => next_attempt_at = Some(deadline),
        }
    }

    let presented = ui_runtime
        .pump(&mut SampledClock(now), &mut sink)
        .presented();

    let mut recovery_check_pending = false;
    let post_attempt = if next_attempt_at.is_none() {
        if let Some(mut guard) = lane.try_lock() {
            if guard.is_device_lost() {
                let attempt =
                    guard.with_backend(|renderer| attempt_device_recovery(renderer, backoff, now));
                if matches!(attempt, RecoveryAttempt::Recovered) {
                    guard.note_surface_recreated();
                }
                Some(attempt)
            } else {
                None
            }
        } else {
            recovery_check_pending = true;
            None
        }
    } else {
        None
    };

    if let Some(attempt) = post_attempt {
        match attempt {
            // Post-frame (mid-frame loss) success arms NO repaint, unlike
            // the pre-frame case above: the frame that just ran already had
            // its own chance to present before the loss was noticed, so
            // there is no known-blank backing store to force a fresh
            // submit for here — forcing one anyway would just be a
            // spurious extra repaint with no correctness reason behind it.
            // The generation re-mint is NOT skippable the same way: the
            // recovery rebuilt the surface either way, and the next frame
            // (whenever something else dirties the tree) must stamp the
            // post-recovery generation.
            RecoveryAttempt::Recovered => {
                backoff.record_success();
                tracing::warn!("GPU device lost — recovered successfully");
            }
            RecoveryAttempt::Failed(error) => {
                just_failed = true;
                next_attempt_at = Some(backoff.record_failure(&error, now));
            }
            RecoveryAttempt::Deferred(deadline) => next_attempt_at = Some(deadline),
        }
    }

    if just_failed || recovery_check_pending {
        // Raised AFTER `render_frame`, never before: that method's
        // own tail (`if retry_needed { wake_frame() } else {
        // mark_rendered() }`) runs unconditionally on EVERY call, and
        // `mark_rendered()` silently clobbers an earlier `wake_frame()`
        // whenever this frame's tree had nothing new to paint
        // (`draw_frame_entered` returns `Idle`, so `retry_needed` stays
        // `false` at the `ui_runtime` layer). That is exactly what happens
        // on every wake AFTER the first against a permanently dead device,
        // once the initial mount's content is already consumed — a
        // pre-frame `wake_frame()` call here self-extinguishes one hop
        // later and the device stays dead for the life of the process.
        // Raising it here, after `render_frame` has already run
        // its own tail, is the only place a failed recovery's wake
        // survives it.
        ui_runtime.wake_frame();
    }

    Some(FrameRecoveryOutcome {
        presented,
        just_failed,
        next_attempt_at,
    })
}

/// The device-loss recovery contract of
/// [`pump_with_device_recovery`] against scripted backends and the
/// [`DeviceRecoveryBackoff`] pacing it: a pre-frame loss recovers BEFORE the
/// frame build and ALWAYS runs it regardless of outcome, a mid-frame loss
/// recovers after, a NEW failure arms the recovery retry wake (never a success,
/// never a merely-deferred attempt), lane contention retains unfinished work,
/// a successful recovery marks the
/// presentation dirty so it actually paints again, and a persistently
/// failing device is retried on a bounded, non-blocking, growing deadline —
/// never abandoned, never slept on.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod device_recovery_tests {
    use flui_engine::PresentDisposition;
    use std::time::Duration;

    use flui_engine::{EngineError, RasterBackend};
    use web_time::Instant;

    use super::{
        DeviceRecovery, DeviceRecoveryBackoff, new_device_recovery_backoff,
        pump_with_device_recovery,
    };

    #[derive(Clone)]
    struct LeafView;

    impl flui_view::RenderView for LeafView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> flui_objects::RenderSizedBox {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            render_object: &mut flui_objects::RenderSizedBox,
        ) -> flui_rendering::RenderUpdateImpact {
            render_object.set_size(Some(0.0), Some(0.0))
        }
    }

    impl flui_view::View for LeafView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    fn mount_root() -> crate::app::ui_runtime::UiRuntime {
        let ui_runtime = crate::app::ui_runtime::UiRuntime::for_test();
        ui_runtime
            .enter(|ui_runtime| ui_runtime.attach_root_widget(&LeafView))
            .expect("attach succeeds");
        ui_runtime
    }

    /// Wraps a scripted backend in the raster lane the production frame
    /// path drives (`pump_with_device_recovery` takes a lane, not
    /// a bare backend, since the desktop/Android runners adopted the
    /// mailbox) — the size matches the scripted backends' own `size()`.
    fn lane_over<B: RasterBackend>(
        backend: B,
    ) -> parking_lot::Mutex<crate::app::raster_lane::RasterLane<B>> {
        parking_lot::Mutex::new(crate::app::raster_lane::RasterLane::new(
            backend,
            flui_foundation::PresentationAddress {
                ui_runtime_id: flui_foundation::UiRuntimeId::new(1),
                presentation_id: flui_foundation::PresentationId::new(1),
            },
            800,
            600,
        ))
    }

    struct ScriptedDeviceBackend {
        /// Current device-lost flag (what `RasterBackend::is_device_lost` reports).
        lost: bool,
        /// `render_scene` outcome once the scene reaches it (`take`n —
        /// `EngineError` is not `Clone`).
        scene_outcome: Option<Result<PresentDisposition, EngineError>>,
        /// `try_recover_device` outcome (`take`n, same reason).
        recover_outcome: Option<Result<(), EngineError>>,
        next_recover_outcome: Option<Result<(), EngineError>>,
        /// Whether a successful recovery clears the lost flag (a failing
        /// driver reset leaves it set).
        recover_clears_lost: bool,
        /// Flip the lost flag from INSIDE `render_scene` — the wgpu
        /// device-lost callback firing mid-frame.
        lose_on_render: bool,
        render_calls: u32,
        recover_attempts: u32,
        resizes: Vec<(u32, u32)>,
    }

    impl ScriptedDeviceBackend {
        fn healthy() -> Self {
            Self {
                lost: false,
                scene_outcome: Some(Ok(PresentDisposition::Presented)),
                recover_outcome: Some(Ok(())),
                next_recover_outcome: None,
                recover_clears_lost: true,
                lose_on_render: false,
                render_calls: 0,
                recover_attempts: 0,
                resizes: Vec::new(),
            }
        }
    }

    impl RasterBackend for ScriptedDeviceBackend {
        fn render_scene(
            &mut self,
            _scene: &flui_layer::Scene,
        ) -> Result<PresentDisposition, EngineError> {
            self.render_calls += 1;
            if self.lose_on_render {
                self.lost = true;
            }
            self.scene_outcome
                .take()
                .expect("render_scene called more than once in a single-frame test")
        }
        fn resize(&mut self, width: u32, height: u32) {
            self.resizes.push((width, height));
        }
        fn is_device_lost(&self) -> bool {
            self.lost
        }
        fn mark_dirty(&mut self, _rect: flui_foundation::geometry::Rect<f64>) {}
        fn mark_full_repaint(&mut self) {}
        fn has_damage(&self) -> bool {
            true
        }
        fn size(&self) -> (u32, u32) {
            (800, 600)
        }
        fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
    }

    impl DeviceRecovery for ScriptedDeviceBackend {
        fn try_recover_device(&mut self) -> Result<(), EngineError> {
            self.recover_attempts += 1;
            let outcome = self
                .recover_outcome
                .take()
                .or_else(|| self.next_recover_outcome.take())
                .expect("every recovery attempt has a scripted result");
            if outcome.is_ok() && self.recover_clears_lost {
                self.lost = false;
            }
            outcome
        }
    }

    fn a_pre_frame_loss_with_a_failing_recovery_still_renders_the_frame_and_backs_off() {
        let mut ui_runtime = mount_root();
        let lane = lane_over(ScriptedDeviceBackend {
            lost: true,
            // The real `Renderer::acquire_surface_texture` bails on the
            // `device_lost` flag before touching the GPU — scripted here
            // as `render_scene` itself reporting `DeviceLost`, since the
            // device is still down when this frame's build reaches it.
            scene_outcome: Some(Err(EngineError::DeviceLost)),
            recover_outcome: Some(Err(EngineError::DeviceLost)),
            recover_clears_lost: false,
            ..ScriptedDeviceBackend::healthy()
        });
        ui_runtime.mark_rendered();
        let backoff = new_device_recovery_backoff();
        let now = Instant::now();

        let outcome = pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, now)
            .expect("idle lane admits the frame");

        assert!(!outcome.presented, "a still-lost device presents nothing");
        assert_eq!(
            lane.lock().with_backend(|b| b.recover_attempts),
            1,
            "the recovery was attempted"
        );
        // The fix this test exists to pin: the earlier version of this
        // function returned `false` here WITHOUT calling
        // `render_frame` at all on a still-lost device, so
        // `render_calls` would read 0. `render_frame` must ALWAYS
        // run — see this module's own doc for why (gesture-arena
        // deadlines, coalesced pointer moves, and every Vsync ticker all
        // depend on it).
        assert_eq!(
            lane.lock().with_backend(|b| b.render_calls),
            1,
            "render_frame must run even when the pre-frame recovery attempt \
             failed — a dead device must not stop the non-GPU half of the frame"
        );
        assert!(outcome.just_failed, "a fresh attempt genuinely failed");
        assert_eq!(
            outcome.next_attempt_at,
            Some(now + DeviceRecoveryBackoff::BASE),
            "the first failed attempt backs off by exactly the base interval"
        );
        assert!(
            ui_runtime.needs_redraw(),
            "the failed recovery must arm the retry wake: on a quiescent loop — no \
             input, no animations — nothing else would ever schedule the next \
             recovery attempt"
        );
    }

    fn a_mid_frame_loss_with_a_failing_recovery_backs_off() {
        let mut ui_runtime = mount_root();
        let lane = lane_over(ScriptedDeviceBackend {
            lose_on_render: true,
            recover_outcome: Some(Err(EngineError::DeviceLost)),
            recover_clears_lost: false,
            ..ScriptedDeviceBackend::healthy()
        });
        ui_runtime.mark_rendered();
        let backoff = new_device_recovery_backoff();
        let now = Instant::now();

        let outcome = pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, now)
            .expect("idle lane admits the frame");

        assert_eq!(
            lane.lock().with_backend(|b| b.render_calls),
            1,
            "the frame rendered before the loss landed"
        );
        assert!(
            outcome.presented,
            "the frame that rendered still reaches present()"
        );
        assert_eq!(
            lane.lock().with_backend(|b| b.recover_attempts),
            1,
            "the mid-frame loss is recovered after the render"
        );
        assert!(
            outcome.just_failed,
            "a fresh post-render attempt genuinely failed"
        );
        assert_eq!(
            outcome.next_attempt_at,
            Some(now + DeviceRecoveryBackoff::BASE),
            "the failed post-render recovery backs off by the base interval, same as a \
             pre-frame failure"
        );
        assert!(
            ui_runtime.needs_redraw(),
            "the post-render recovery wake must survive render_frame's own \
             mark_rendered(), so the recovered renderer renders again on a quiescent \
             loop"
        );
    }

    /// Direct reproduction of the review's self-extinguishing-retry finding:
    /// with `wake_frame()` raised INSIDE the recovery attempt (BEFORE
    /// `render_frame` runs, the shape this test would fail
    /// against), `render_frame`'s own tail (`mark_rendered()`,
    /// since a quiescent tree with nothing new to paint produces `Idle`,
    /// not `Errored`) silently clobbers it one hop later. A two-frame drive
    /// does not catch this: the FIRST frame still has the mount's own
    /// pending paint, reaches `render_scene`, and `ui_runtime`'s OWN
    /// `DeviceLost` arm independently arms `needs_redraw` too (`retry_needed
    /// = true` there, from this same issue's `ui_runtime/` fix) — the bug
    /// only shows up once that leftover demand is exhausted, on the frame
    /// AFTER, which is exactly why this drives (and checks) three.
    fn needs_redraw_stays_armed_across_three_consecutive_frames_against_a_permanently_dead_device()
    {
        let mut ui_runtime = mount_root();
        let backoff = new_device_recovery_backoff();
        let mut now = Instant::now();

        for frame in 1..=3u32 {
            let lane = lane_over(ScriptedDeviceBackend {
                lost: true,
                scene_outcome: Some(Err(EngineError::DeviceLost)),
                recover_outcome: Some(Err(EngineError::DeviceLost)),
                recover_clears_lost: false,
                ..ScriptedDeviceBackend::healthy()
            });
            let _ = pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, now)
                .expect("idle lane admits the frame");
            assert!(
                ui_runtime.needs_redraw(),
                "needs_redraw must still be armed after frame {frame} against a \
                 permanently dead device — a device that never recovers must never let \
                 this flag settle back to false, or nothing will ever wake the loop to \
                 retry again"
            );
            // Space frames out past the backoff's own growing deadline so
            // each one is a genuine NEW attempt, not one silently deferred
            // by the backoff (which would make this loop pass trivially,
            // for the wrong reason — a deferred attempt reports no fresh
            // failure at all).
            now += Duration::from_secs(2);
        }
    }

    fn frame_callbacks_run_without_the_raster_lane_guard() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::Arc;

        let mut ui_runtime = mount_root();
        let lane = Arc::new(lane_over(ScriptedDeviceBackend::healthy()));
        let observations = Rc::new(RefCell::new(Vec::new()));
        let transient_lane = Arc::clone(&lane);
        let transient_observations = Rc::clone(&observations);
        ui_runtime
            .scheduler()
            .schedule_frame_callback(Box::new(move |_| {
                let hook = transient_lane.try_lock().map(|lane| lane.resize_hook());
                let available = hook.is_some();
                if let Some(hook) = hook {
                    hook.apply(900, 700);
                }
                transient_observations
                    .borrow_mut()
                    .push(("transient", available));
            }));
        let post_lane = Arc::clone(&lane);
        let post_observations = Rc::clone(&observations);
        ui_runtime
            .scheduler()
            .add_post_frame_callback(Box::new(move |_| {
                let hook = post_lane.try_lock().map(|lane| lane.resize_hook());
                let available = hook.is_some();
                if let Some(hook) = hook {
                    hook.apply(1200, 800);
                }
                post_observations
                    .borrow_mut()
                    .push(("post-frame", available));
            }));

        let backoff = new_device_recovery_backoff();
        let outcome = pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
            .expect("idle lane admits the frame");
        assert_eq!(
            *observations.borrow(),
            [("transient", true), ("post-frame", true)],
            "owner callbacks must run once and without the raster infrastructure guard"
        );
        assert!(outcome.presented, "the same pump still presents its scene");
        lane.lock().with_backend(|backend| {
            assert_eq!(backend.render_calls, 1);
            assert_eq!(backend.resizes, [(900, 700)]);
            backend.scene_outcome = Some(Ok(PresentDisposition::Presented));
        });
        ui_runtime.mark_primary_needs_full_repaint();
        assert!(
            pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
                .expect("idle lane admits the next frame")
                .presented
        );
        lane.lock().with_backend(|backend| {
            assert_eq!(backend.render_calls, 2);
            assert_eq!(backend.resizes, [(900, 700), (1200, 800)]);
        });
    }

    fn a_busy_lane_keeps_the_unstarted_frame_deliverable() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut ui_runtime = mount_root();
        let lane = lane_over(ScriptedDeviceBackend::healthy());
        let calls = Rc::new(Cell::new(0));
        let callback_calls = Rc::clone(&calls);
        ui_runtime
            .scheduler()
            .schedule_frame_callback(Box::new(move |_| {
                callback_calls.set(callback_calls.get() + 1);
            }));
        let backoff = new_device_recovery_backoff();
        let guard = lane.lock();
        assert!(
            pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now()).is_none()
        );
        assert_eq!(calls.get(), 0, "no scheduler phase was consumed");
        assert!(ui_runtime.needs_redraw());
        assert_eq!(backoff.next_attempt_at(), None);
        drop(guard);
        assert!(
            pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
                .expect("released lane admits retained work")
                .presented
        );
        assert_eq!(calls.get(), 1);
    }

    fn lane_contention_during_pump(post_frame: bool) {
        use std::sync::{Arc, mpsc};

        let mut ui_runtime = mount_root();
        let lane = Arc::new(lane_over(ScriptedDeviceBackend::healthy()));
        let worker_lane = Arc::clone(&lane);
        let (start_tx, start_rx) = mpsc::channel::<()>();
        let (held_tx, held_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            start_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("frame starts the competing operation");
            let mut guard = worker_lane.lock();
            if post_frame {
                guard.with_backend(|backend| {
                    backend.lost = true;
                    backend.scene_outcome = Some(Ok(PresentDisposition::Presented));
                });
            }
            held_tx.send(()).expect("frame observes lane contention");
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
            drop(guard);
        });
        let hold_lane = move || {
            start_tx.send(()).expect("competing operation starts");
            held_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("competing operation holds the lane without waiting on the frame");
        };
        if post_frame {
            ui_runtime
                .scheduler()
                .add_post_frame_callback(Box::new(move |_| hold_lane()));
        } else {
            ui_runtime
                .scheduler()
                .schedule_frame_callback(Box::new(move |_| hold_lane()));
        }
        let backoff = new_device_recovery_backoff();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
        }));
        let _ = release_tx.send(());
        let worker_result = worker.join();
        let outcome = match outcome {
            Ok(outcome) => outcome.expect("the lane was available at frame admission"),
            Err(payload) => std::panic::resume_unwind(payload),
        };
        worker_result.expect("competing operation returns normally");
        assert_eq!(outcome.presented, post_frame);
        assert!(!outcome.just_failed, "contention is not device loss");
        assert_eq!(outcome.next_attempt_at, None);
        assert_eq!(backoff.next_attempt_at(), None);
        assert!(
            ui_runtime.needs_redraw(),
            "unfinished frame work remains owed"
        );
        lane.lock().with_backend(|backend| {
            assert_eq!(backend.render_calls, u32::from(post_frame));
            assert_eq!(backend.recover_attempts, 0);
        });
        assert!(
            pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
                .expect("released lane admits the repaint")
                .presented
        );
        lane.lock().with_backend(|backend| {
            assert_eq!(backend.render_calls, 1 + u32::from(post_frame));
            assert_eq!(backend.recover_attempts, u32::from(post_frame));
        });
    }

    fn mid_frame_lane_contention_retries_the_scene_and_recovery_check() {
        lane_contention_during_pump(false);
    }

    fn post_frame_lane_contention_keeps_device_recovery_deliverable() {
        lane_contention_during_pump(true);
    }

    fn pre_frame_recovery_repaints_an_already_clean_tree() {
        let mut ui_runtime = mount_root();
        let lane = lane_over(ScriptedDeviceBackend::healthy());
        let backoff = new_device_recovery_backoff();
        assert!(
            pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
                .expect("initial frame is admitted")
                .presented
        );
        lane.lock().with_backend(|backend| {
            backend.lost = true;
            backend.scene_outcome = Some(Ok(PresentDisposition::Presented));
        });
        let outcome = pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, Instant::now())
            .expect("recovery frame is admitted");
        assert!(outcome.presented, "recovery dirties the clean producer");
        assert!(!outcome.just_failed);
        assert_eq!(backoff.next_attempt_at(), None);
        lane.lock().with_backend(|backend| {
            assert_eq!(backend.render_calls, 2);
            assert_eq!(backend.recover_attempts, 1);
        });
    }

    fn a_recovered_then_relost_device_is_retried_after_the_same_pump() {
        let mut ui_runtime = mount_root();
        let lane = lane_over(ScriptedDeviceBackend {
            lost: true,
            lose_on_render: true,
            next_recover_outcome: Some(Err(EngineError::DeviceLost)),
            ..ScriptedDeviceBackend::healthy()
        });
        let backoff = new_device_recovery_backoff();
        let now = Instant::now();
        let outcome = pump_with_device_recovery(&mut ui_runtime, &lane, &backoff, now)
            .expect("recovery frame is admitted");
        assert!(outcome.presented);
        assert!(outcome.just_failed);
        assert_eq!(
            outcome.next_attempt_at,
            Some(now + DeviceRecoveryBackoff::BASE)
        );
        assert!(ui_runtime.needs_redraw());
        lane.lock().with_backend(|backend| {
            assert_eq!(backend.render_calls, 1, "only one pump produced a scene");
            assert_eq!(
                backend.recover_attempts, 2,
                "recovery brackets that same pump"
            );
        });
    }

    #[test]
    fn device_recovery_matrix() {
        crate::table_test::run_table(
            "device_recovery_matrix",
            &[
                ("a_busy_lane_keeps_the_unstarted_frame_deliverable", a_busy_lane_keeps_the_unstarted_frame_deliverable as fn()),
                ("mid_frame_lane_contention_retries_the_scene_and_recovery_check", mid_frame_lane_contention_retries_the_scene_and_recovery_check as fn()),
                ("post_frame_lane_contention_keeps_device_recovery_deliverable", post_frame_lane_contention_keeps_device_recovery_deliverable as fn()),
                ("pre_frame_recovery_repaints_an_already_clean_tree", pre_frame_recovery_repaints_an_already_clean_tree as fn()),
                ("a_recovered_then_relost_device_is_retried_after_the_same_pump", a_recovered_then_relost_device_is_retried_after_the_same_pump as fn()),
                ("frame_callbacks_run_without_the_raster_lane_guard", frame_callbacks_run_without_the_raster_lane_guard as fn()),
                ("a_pre_frame_loss_with_a_failing_recovery_still_renders_the_frame_and_backs_off", a_pre_frame_loss_with_a_failing_recovery_still_renders_the_frame_and_backs_off as fn()),
                ("a_mid_frame_loss_with_a_failing_recovery_backs_off", a_mid_frame_loss_with_a_failing_recovery_backs_off as fn()),
                ("needs_redraw_stays_armed_across_three_consecutive_frames_against_a_permanently_dead_device", needs_redraw_stays_armed_across_three_consecutive_frames_against_a_permanently_dead_device as fn()),
            ],
        );
    }
}
