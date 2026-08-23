#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
use super::frame_pacing::NO_PRESENT_FALLBACK_PACE;

// ============================================================================
// GPU device-loss recovery (App.1 device-recovery wake)
// ============================================================================
//
// The sync device-rebuild seam plus the shared retry/backoff logic the
// desktop and Android frame drivers both use around
// `UiRealm::render_frame_entered`. Web's own recovery (`bootstrap_web`)
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
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
pub(super) trait DeviceRecovery {
    /// Attempt to rebuild the lost device synchronously on the runner
    /// thread.
    fn try_recover_device(&mut self) -> Result<(), flui_engine::EngineError>;
}

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
impl DeviceRecovery for flui_engine::wgpu::Renderer {
    fn try_recover_device(&mut self) -> Result<(), flui_engine::EngineError> {
        // `pollster` is already a dep and safe to use here — the
        // desktop/Android runners own synchronous platform callbacks, not
        // an async executor.
        pollster::block_on(self.recover())
    }
}

/// Exponential backoff for the [`DeviceRecovery::try_recover_device`] retry
/// loop: paces how often an ATTEMPT is made while the device stays lost,
/// growing from one frame interval up to a capped ceiling and resetting on
/// the first successful recovery.
///
/// Deliberately never gives up permanently (no attempt-count ceiling):
/// device loss is expected to clear eventually on every platform this runs
/// on — a driver TDR reset, an app coming back from the background, an
/// eGPU replug — so a hard cap would turn a recoverable condition into a
/// dead app. An UNBOUNDED, un-backed-off retry loop is equally wrong (a
/// full `recover()` rebuilds the instance/adapter/device/surface/painter/
/// offscreen target — not cheap to repeat every wake), so this paces the
/// ATTEMPT itself, not just the log line.
///
/// This is a DEADLINE, not a sleep: [`Self::next_attempt_at`] reports the
/// earliest instant an attempt is allowed, and the caller's only obligation
/// is to skip attempting before it — never to block the calling thread
/// waiting for it. A `thread::sleep` sized to this backoff's own interval
/// (climbing to a full second at the cap) was tried and rejected: on the
/// platform event-loop thread that call runs on, it blocks input dispatch
/// for its duration on every backend, and on Android specifically it also
/// blocks `MainEvent` lifecycle delivery (`AndroidPlatform::run`'s poll
/// loop calls `process_input_events`/`dispatch_request_frame` inline, on
/// the SAME thread) — repeated one-second stalls there are ANR territory.
/// Both desktop and Android wire [`DeviceRecoveryBackoff::next_attempt_at`]
/// into a wall-clock wake-deadline hook (`install_wake_deadline_hook` for
/// desktop; `Platform::set_wake_deadline_hook` on `AndroidPlatform` directly
/// for Android, added alongside this backoff since the trait's default
/// implementation is a no-op there) so the platform's own idle wait — or,
/// on Android, its own already-running ~16ms idle poll — carries the
/// deadline instead of this crate sleeping on it. See each hook's own doc
/// for exactly what it can express and at what granularity. **A deadline
/// wired into either hook is necessary but not sufficient on its own**: it
/// must also appear in the owning frame closure's `dirty` predicate (`wake_
/// action`'s input) or the wake it actuates reaches `WakeAction::Skip` and
/// returns before this backoff is ever consulted again — both closures'
/// `dirty` computations OR in `next_attempt_at().is_some()` for exactly
/// this reason.
///
/// `Send + Sync`: captured behind an `Arc` by the platform's
/// `on_request_frame` callback, which every backend's `Window::
/// on_request_frame` requires to be `Send` (`flui-platform/src/traits/
/// window.rs`). State lives behind one `parking_lot::Mutex` rather than
/// several independent atomics — this is read/written at most once per
/// frame wake, and a single lock rules out the counters and the deadline
/// ever being updated out of step with each other.
///
/// This mutex has TWO callers, not one: the frame closure itself
/// (`attempt_device_recovery`/[`Self::record_failure`]/[`Self::record_success`])
/// and the wake-deadline hook closure ([`Self::next_attempt_at`], called
/// from desktop's `about_to_wait` or Android's own poll loop). Both run on
/// this platform's single event-loop thread, never concurrently with each
/// other — that same-thread invariant is what makes holding this
/// `parking_lot::Mutex` (non-reentrant: a same-thread re-lock while already
/// held deadlocks, it does not block-and-wait) safe with no actual
/// contention today. It is still deliberately never held across a
/// `tracing` call (whose subscriber may perform I/O) — see
/// [`Self::record_failure`]'s own comment for where the guard is dropped
/// before logging, defensively, not because a reentrant call from within a
/// subscriber has been observed.
///
/// [`web_time::Instant`], not `std::time::Instant`: a deliberate match to
/// every other clock on this frame-driver path (`web_time::Instant::now()`
/// at the top of both closures, `AppRuntime::next_wake`'s own return type)
/// rather than an accident of this type compiling only because the two
/// happen to be the same type off-`wasm32` (this backoff's own `cfg` gate
/// already excludes `wasm32`, so the distinction is moot today, but the
/// convention is the same one this whole module already follows).
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
pub(super) struct DeviceRecoveryBackoff {
    state: parking_lot::Mutex<DeviceRecoveryBackoffState>,
}

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
struct DeviceRecoveryBackoffState {
    /// Consecutive failures since the last success (or since construction).
    consecutive_failures: u32,
    /// Whether the "backoff reached its cap" error has already been logged
    /// once this losing streak — re-armed on the next success.
    cap_logged: bool,
    /// The earliest instant the next attempt is allowed. `None` before the
    /// first failure of a streak (attempt immediately) or right after a
    /// success.
    next_attempt_at: Option<web_time::Instant>,
}

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
impl DeviceRecoveryBackoff {
    /// The base interval: the same cadence [`NO_PRESENT_FALLBACK_PACE`]
    /// already paces a no-present frame at, reused rather than inventing a
    /// second pacing constant for the same "roughly one frame interval"
    /// shape.
    const BASE: std::time::Duration = NO_PRESENT_FALLBACK_PACE;
    /// The ceiling: "on the order of a second", per this module's device-
    /// recovery retry policy.
    const CAP: std::time::Duration = std::time::Duration::from_secs(1);
    /// `BASE << SHIFT_CAP >= CAP` already holds well before this shift is
    /// reached, so capping the shift itself (rather than only the final
    /// `.min(CAP)`) avoids ever computing `1u32 << n` for an
    /// unboundedly long losing streak.
    const SHIFT_CAP: u32 = 6;

    pub(super) fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(DeviceRecoveryBackoffState {
                consecutive_failures: 0,
                cap_logged: false,
                next_attempt_at: None,
            }),
        }
    }

    /// The earliest instant the next attempt is allowed, if a failure has
    /// armed one. `None` when ready right now (never failed, or the last
    /// outcome was a success).
    pub(super) fn next_attempt_at(&self) -> Option<web_time::Instant> {
        self.state.lock().next_attempt_at
    }

    /// Record a failed recovery attempt at `now`, arm the next deadline,
    /// and return it.
    ///
    /// Logs the first failure of a losing streak at `error`, every
    /// subsequent one at `debug`, and re-emits `error` exactly once more
    /// when the backoff reaches [`Self::CAP`] — a permanently dead GPU
    /// says so once more at that point, not on every attempt after it.
    fn record_failure(
        &self,
        error: &flui_engine::EngineError,
        now: web_time::Instant,
    ) -> web_time::Instant {
        /// Which line to log, decided while the state lock is held (it
        /// reads/mutates `cap_logged`); the actual `tracing` call happens
        /// AFTER the guard drops — see this method's own call site below.
        enum LogKind {
            FirstFailure,
            ReachedCap,
            Retrying,
        }

        let (deadline, interval, log_kind) = {
            let mut state = self.state.lock();
            let shift = state.consecutive_failures.min(Self::SHIFT_CAP);
            let interval = (Self::BASE * (1u32 << shift)).min(Self::CAP);
            let at_cap = interval >= Self::CAP;
            let is_first = state.consecutive_failures == 0;
            state.consecutive_failures += 1;
            let deadline = now + interval;
            state.next_attempt_at = Some(deadline);

            let log_kind = if is_first {
                LogKind::FirstFailure
            } else if at_cap && !state.cap_logged {
                state.cap_logged = true;
                LogKind::ReachedCap
            } else {
                LogKind::Retrying
            };
            (deadline, interval, log_kind)
            // `state` (the `MutexGuard`) drops here, before any `tracing`
            // call: this mutex's other caller is the wake-deadline hook
            // closure (see this type's own doc for why holding it across a
            // subscriber's possible I/O is undesirable even though no
            // reentrant call is reachable today).
        };

        match log_kind {
            LogKind::FirstFailure => {
                tracing::error!(error = ?error, "GPU device recovery failed; retrying with backoff");
            }
            LogKind::ReachedCap => {
                tracing::error!(
                    error = ?error,
                    backoff = ?interval,
                    "GPU device recovery still failing at the backoff cap; retries continue \
                     silently from here"
                );
            }
            LogKind::Retrying => {
                tracing::debug!(
                    error = ?error,
                    backoff = ?interval,
                    "GPU device recovery failed; retrying"
                );
            }
        }
        deadline
    }

    /// Reset the backoff after a successful recovery.
    fn record_success(&self) {
        let mut state = self.state.lock();
        state.consecutive_failures = 0;
        state.cap_logged = false;
        state.next_attempt_at = None;
    }
}

/// Outcome of one call to [`attempt_device_recovery`].
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
enum RecoveryAttempt {
    /// The backoff's armed deadline had not yet elapsed — no attempt was
    /// made. Carries that SAME deadline (not a freshly computed one).
    Deferred(web_time::Instant),
    /// A fresh attempt was made and failed. Carries the NEW deadline the
    /// failure just armed.
    Failed(web_time::Instant),
    /// A fresh attempt was made and succeeded.
    Recovered,
}

/// Attempt one synchronous device rebuild — but only if `backoff`'s deadline
/// has elapsed; otherwise returns immediately without touching the renderer
/// or the backoff's counters. See [`DeviceRecoveryBackoff`]'s own doc for
/// why this is a deadline CHECK, never a sleep: skipping is the only
/// non-blocking way to pace an attempt that can cost a full GPU stack
/// rebuild.
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
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
        Ok(()) => {
            tracing::warn!("GPU device lost — recovered successfully");
            backoff.record_success();
            RecoveryAttempt::Recovered
        }
        Err(e) => {
            let deadline = backoff.record_failure(&e, now);
            RecoveryAttempt::Failed(deadline)
        }
    }
}

/// Outcome of driving one frame through [`render_frame_with_device_recovery`].
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
pub(super) struct FrameRecoveryOutcome {
    /// Whether the frame reached `present()` — same meaning as
    /// [`crate::app::ui_realm::UiRealm::render_frame_entered`]'s own return.
    pub(super) presented: bool,
    /// Set exactly when a NEW recovery attempt failed this call — never on
    /// a merely-deferred attempt (the backoff deadline had not elapsed) and
    /// never on success. Only this arms the retry wake; see this function's
    /// own doc for why raising it here, not inside the attempt helper
    /// itself, is what makes the wake survive.
    ///
    /// Read only by this module's own tests today: production callers
    /// (`bootstrap_desktop`/`bootstrap_android`) consult the persistent
    /// `DeviceRecoveryBackoff` directly (via its own `next_attempt_at`) for
    /// the wake-deadline hook rather than this per-call snapshot, so this
    /// field carries no separate production obligation of its own — kept
    /// on the struct anyway because it is what makes the wake-on-failure-
    /// only contract independently checkable per call.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read by this module's own tests only -- see field doc"
        )
    )]
    just_failed: bool,
    /// The earliest instant the next recovery attempt is allowed, if the
    /// device is still (or newly) lost. `None` once healthy.
    ///
    /// Read only by this module's own tests today, for the same reason as
    /// [`Self::just_failed`] — production callers consult
    /// `DeviceRecoveryBackoff::next_attempt_at` directly instead.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read by this module's own tests only -- see field doc"
        )
    )]
    next_attempt_at: Option<web_time::Instant>,
}

/// Drive one frame through `realm` against `renderer`, rebuilding a lost
/// device around it.
///
/// A device already lost at frame start gets a backoff-gated recovery
/// attempt here, BEFORE [`crate::app::ui_realm::UiRealm::render_frame_entered`]
/// — but `render_frame_entered` runs regardless of whether that attempt
/// happened or what it returned. Skipping it on a still-lost device was the
/// original bug this function fixes: `render_frame_entered` is the only
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
/// `next_attempt_at` at `None`, so if the SAME call's `render_frame_entered`
/// then loses the device again, the post-frame check still fires — a
/// `was_lost_pre_frame`-style gate would silently miss exactly that
/// recovered-then-re-lost-mid-frame case, since it only ever asks about the
/// PRE-frame state. Either check finding a non-`None` `next_attempt_at`
/// means a decision (failed or deferred) was already recorded THIS call, so
/// the other one skips — attempting twice in one wake would silently double
/// the effective retry rate the backoff exists to bound.
///
/// A SUCCESSFUL PRE-frame attempt calls [`crate::app::ui_realm::UiRealm::
/// mark_primary_needs_full_repaint`] — never [`crate::app::ui_realm::UiRealm::
/// wake_frame`], and never bare [`crate::app::ui_realm::UiRealm::request_redraw`]
/// either. Neither of those alone is enough: `wake_frame` does not open
/// `render_frame_entered`'s OWN per-presentation dirty gate
/// (`presentation.take_redraw_pending()`, `ui_realm.rs`'s
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
/// failure): this call's OWN `render_frame_entered` below runs
/// synchronously right after, in the very same wake, so there is nothing
/// external left to wake. The POST-frame (mid-frame-loss) success arm calls
/// neither: that frame already had its own chance to present before the
/// loss was even noticed, so there is no known-blank backing store to force
/// a fresh submit for.
#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
pub(super) fn render_frame_with_device_recovery<R>(
    realm: &crate::app::ui_realm::UiRealm,
    renderer: &mut R,
    backoff: &DeviceRecoveryBackoff,
    now: web_time::Instant,
) -> FrameRecoveryOutcome
where
    R: flui_engine::RasterBackend + DeviceRecovery,
{
    let mut just_failed = false;
    let mut next_attempt_at = None;

    if renderer.is_device_lost() {
        match attempt_device_recovery(renderer, backoff, now) {
            // Pre-frame only: nothing has presented since the device died,
            // so the recovered backing store needs a genuinely fresh
            // submit — see `UiRealm::mark_primary_needs_full_repaint`'s
            // own doc for why `request_redraw` alone does not get one.
            RecoveryAttempt::Recovered => realm.mark_primary_needs_full_repaint(),
            RecoveryAttempt::Failed(deadline) => {
                just_failed = true;
                next_attempt_at = Some(deadline);
            }
            RecoveryAttempt::Deferred(deadline) => next_attempt_at = Some(deadline),
        }
    }

    let presented = realm.render_frame_entered(renderer);

    if next_attempt_at.is_none() && renderer.is_device_lost() {
        match attempt_device_recovery(renderer, backoff, now) {
            // Post-frame (mid-frame loss) success arms NOTHING, unlike the
            // pre-frame case above: the frame that just ran already had
            // its own chance to present before the loss was noticed, so
            // there is no known-blank backing store to force a fresh
            // submit for here — forcing one anyway would just be a
            // spurious extra repaint with no correctness reason behind it.
            RecoveryAttempt::Recovered => {}
            RecoveryAttempt::Failed(deadline) => {
                just_failed = true;
                next_attempt_at = Some(deadline);
            }
            RecoveryAttempt::Deferred(deadline) => next_attempt_at = Some(deadline),
        }
    }

    if just_failed {
        // Raised AFTER `render_frame_entered`, never before: that method's
        // own tail (`if retry_needed { wake_frame() } else {
        // mark_rendered() }`) runs unconditionally on EVERY call, and
        // `mark_rendered()` silently clobbers an earlier `wake_frame()`
        // whenever this frame's tree had nothing new to paint
        // (`draw_frame_entered` returns `Idle`, so `retry_needed` stays
        // `false` at the `ui_realm` layer). That is exactly what happens
        // on every wake AFTER the first against a permanently dead device,
        // once the initial mount's content is already consumed — a
        // pre-frame `wake_frame()` call here self-extinguishes one hop
        // later and the device stays dead for the life of the process.
        // Raising it here, after `render_frame_entered` has already run
        // its own tail, is the only place a failed recovery's wake
        // survives it.
        realm.wake_frame();
    }

    FrameRecoveryOutcome {
        presented,
        just_failed,
        next_attempt_at,
    }
}

/// The device-loss recovery contract of
/// [`render_frame_with_device_recovery`] against scripted backends and the
/// [`DeviceRecoveryBackoff`] pacing it: a pre-frame loss recovers BEFORE the
/// frame build and ALWAYS runs it regardless of outcome, a mid-frame loss
/// recovers after, only a NEW failure arms the retry wake (never a success,
/// never a merely-deferred attempt), a successful recovery marks the
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
    use std::time::Duration;

    use flui_engine::{EngineError, RasterBackend};
    use web_time::Instant;

    use super::super::frame_pacing::frame_is_dirty;
    use super::{DeviceRecovery, DeviceRecoveryBackoff, render_frame_with_device_recovery};

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
            render_object.set_size(
                Some(flui_types::Pixels::ZERO),
                Some(flui_types::Pixels::ZERO),
            )
        }
    }

    impl flui_view::View for LeafView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    fn mount_root() -> crate::app::ui_realm::UiRealm {
        let realm = crate::app::ui_realm::UiRealm::for_test();
        realm
            .enter(|realm| realm.attach_root_widget(&LeafView))
            .expect("attach succeeds");
        realm
    }

    struct ScriptedDeviceBackend {
        /// Current device-lost flag (what `RasterBackend::is_device_lost` reports).
        lost: bool,
        /// `render_scene` outcome once the scene reaches it (`take`n —
        /// `EngineError` is not `Clone`).
        scene_outcome: Option<Result<bool, EngineError>>,
        /// `try_recover_device` outcome (`take`n, same reason).
        recover_outcome: Option<Result<(), EngineError>>,
        /// Whether a successful recovery clears the lost flag (a failing
        /// driver reset leaves it set).
        recover_clears_lost: bool,
        /// Flip the lost flag from INSIDE `render_scene` — the wgpu
        /// device-lost callback firing mid-frame.
        lose_on_render: bool,
        render_calls: u32,
        recover_attempts: u32,
    }

    impl ScriptedDeviceBackend {
        fn healthy() -> Self {
            Self {
                lost: false,
                scene_outcome: Some(Ok(true)),
                recover_outcome: Some(Ok(())),
                recover_clears_lost: true,
                lose_on_render: false,
                render_calls: 0,
                recover_attempts: 0,
            }
        }
    }

    impl RasterBackend for ScriptedDeviceBackend {
        fn render_scene(&mut self, _scene: &flui_layer::Scene) -> Result<bool, EngineError> {
            self.render_calls += 1;
            if self.lose_on_render {
                self.lost = true;
            }
            self.scene_outcome
                .take()
                .expect("render_scene called more than once in a single-frame test")
        }
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn is_device_lost(&self) -> bool {
            self.lost
        }
        fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
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
                .expect("try_recover_device called more than once in a single-frame test");
            if outcome.is_ok() && self.recover_clears_lost {
                self.lost = false;
            }
            outcome
        }
    }

    #[test]
    fn a_healthy_device_renders_without_any_recovery() {
        let realm = mount_root();
        let mut backend = ScriptedDeviceBackend::healthy();
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert!(outcome.presented, "Ok(true) reaches present()");
        assert_eq!(backend.render_calls, 1, "the scene reached render_scene");
        assert_eq!(
            backend.recover_attempts, 0,
            "no recovery on a healthy device"
        );
        assert!(
            !outcome.just_failed,
            "no recovery attempt failed, so nothing arms the retry wake"
        );
        assert!(
            outcome.next_attempt_at.is_none(),
            "a healthy device has no pending recovery deadline"
        );
        assert!(
            !realm.needs_redraw(),
            "a successful frame clears the redraw flag"
        );
    }

    #[test]
    fn a_pre_frame_loss_with_a_successful_recovery_renders_the_same_frame_and_arms_nothing() {
        let realm = mount_root();
        let mut backend = ScriptedDeviceBackend {
            lost: true,
            ..ScriptedDeviceBackend::healthy()
        };
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert_eq!(
            backend.recover_attempts, 1,
            "the pre-frame loss is recovered"
        );
        assert!(
            !backend.lost,
            "the scripted successful recovery cleared the lost flag"
        );
        assert_eq!(
            backend.render_calls, 1,
            "after a successful recovery the SAME frame renders — no extra wake needed"
        );
        assert!(outcome.presented, "the recovered frame reaches present()");
        assert!(
            !outcome.just_failed,
            "a SUCCESSFUL recovery must not report a failure — there is nothing to retry"
        );
        assert!(
            outcome.next_attempt_at.is_none(),
            "a successful recovery leaves no pending deadline"
        );
        // The original bug this pins: an earlier version of this test
        // never asserted `needs_redraw()`, which is exactly where a
        // spurious wake on the success arm hid — a successful pre-frame
        // recovery is immediately followed by rendering this very frame,
        // and that render's own `mark_rendered()` would silently clobber
        // an extra `wake_frame()` two statements later, leaving only a
        // wasted platform `request_redraw` with no test ever catching it.
        assert!(
            !realm.needs_redraw(),
            "a successful pre-frame recovery followed by a presented frame must leave no \
             spurious wake armed"
        );
    }

    #[test]
    fn a_successful_recovery_forces_the_recovered_devices_next_frame_to_actually_paint() {
        let realm = mount_root();
        let mut backend = ScriptedDeviceBackend::healthy();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        // Consume the mount's own pending paint FIRST, so the presentation's
        // wake-only redraw bit (`PresentationState::mark_redraw_pending`,
        // read by `draw_frame_entered`'s `take_redraw_pending`) is
        // definitely false going into the recovery below — otherwise this
        // test could pass by riding the mount's own leftover demand
        // instead of proving what the recovery success arm itself does.
        let warm_up = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
        assert!(
            warm_up.presented,
            "precondition: the mount's own first frame presented"
        );
        assert_eq!(backend.render_calls, 1);

        // Now the device dies and recovers, with NOTHING else marking the
        // tree dirty in between (no input, no animation, no resize).
        backend.lost = true;
        backend.scene_outcome = Some(Ok(true));
        backend.recover_outcome = Some(Ok(()));

        let recovery = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        // The fix this test pins: `wake_frame()` alone (the realm-level
        // flag plus a platform poke) does NOT set `PresentationState::
        // redraw_pending` — only `UiRealm::request_redraw` does, and that
        // bit is what `draw_frame_entered`'s segment gate actually reads.
        // Without marking it on a SUCCESSFUL recovery, this second call
        // would find nothing dirty, `draw_frame_entered` would return
        // `Idle`, and `render_scene` would never be reached — the
        // recovered device would stay visually blank (its own backing
        // store was invalidated by the loss; `Renderer::recover` primes a
        // full repaint for whatever gets submitted next, but nothing
        // submits at all without this).
        assert_eq!(
            backend.render_calls, 2,
            "a successful recovery must force the recovered device's next frame to reach \
             render_scene, not silently stay Idle"
        );
        assert!(
            recovery.presented,
            "the forced repaint must reach present()"
        );
    }

    #[test]
    fn a_pre_frame_loss_with_a_failing_recovery_still_renders_the_frame_and_backs_off() {
        let realm = mount_root();
        let mut backend = ScriptedDeviceBackend {
            lost: true,
            // The real `Renderer::acquire_surface_texture` bails on the
            // `device_lost` flag before touching the GPU — scripted here
            // as `render_scene` itself reporting `DeviceLost`, since the
            // device is still down when this frame's build reaches it.
            scene_outcome: Some(Err(EngineError::DeviceLost)),
            recover_outcome: Some(Err(EngineError::DeviceLost)),
            recover_clears_lost: false,
            ..ScriptedDeviceBackend::healthy()
        };
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert!(!outcome.presented, "a still-lost device presents nothing");
        assert_eq!(backend.recover_attempts, 1, "the recovery was attempted");
        // The fix this test exists to pin: the earlier version of this
        // function returned `false` here WITHOUT calling
        // `render_frame_entered` at all on a still-lost device, so
        // `render_calls` would read 0. `render_frame_entered` must ALWAYS
        // run — see this module's own doc for why (gesture-arena
        // deadlines, coalesced pointer moves, and every Vsync ticker all
        // depend on it).
        assert_eq!(
            backend.render_calls, 1,
            "render_frame_entered must run even when the pre-frame recovery attempt \
             failed — a dead device must not stop the non-GPU half of the frame"
        );
        assert!(outcome.just_failed, "a fresh attempt genuinely failed");
        assert_eq!(
            outcome.next_attempt_at,
            Some(now + DeviceRecoveryBackoff::BASE),
            "the first failed attempt backs off by exactly the base interval"
        );
        assert!(
            realm.needs_redraw(),
            "the failed recovery must arm the retry wake: on a quiescent loop — no \
             input, no animations — nothing else would ever schedule the next \
             recovery attempt"
        );
    }

    #[test]
    fn a_deferred_attempt_before_the_deadline_does_not_touch_the_renderer() {
        let realm = mount_root();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let mut backend = ScriptedDeviceBackend {
            lost: true,
            scene_outcome: Some(Err(EngineError::DeviceLost)),
            recover_outcome: Some(Err(EngineError::DeviceLost)),
            recover_clears_lost: false,
            ..ScriptedDeviceBackend::healthy()
        };
        let first = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
        assert!(
            first.just_failed,
            "precondition: the first attempt genuinely failed"
        );
        assert_eq!(backend.recover_attempts, 1);

        // Refilled so a genuine second attempt, if this test's premise is
        // wrong, fails loudly on its own assertions below rather than
        // panicking on a `.take()` of `None`.
        backend.scene_outcome = Some(Err(EngineError::DeviceLost));
        backend.recover_outcome = Some(Err(EngineError::DeviceLost));

        // Same instant, well before the backoff's own deadline (BASE, 16ms).
        let second = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert_eq!(
            backend.recover_attempts, 1,
            "a wake before the backoff's deadline must not touch the renderer at all — the \
             whole point of a deadline CHECK over a sleep is that a still-too-early wake \
             costs nothing"
        );
        assert!(
            !second.just_failed,
            "a deferred (not attempted) call must not report a fresh failure"
        );
        assert_eq!(
            second.next_attempt_at, first.next_attempt_at,
            "a deferred call must report the SAME deadline the earlier failure armed, not \
             a freshly computed one"
        );
    }

    /// A backend that recovers successfully on EVERY `try_recover_device`
    /// call (no `.take()`-once panic guard — this test's whole point is
    /// calling it twice) and dies again during `render_scene` exactly
    /// once, right after the first recovery cleared `lost`.
    struct AlwaysRecoversButDiesOnFirstRenderBackend {
        lost: bool,
        died_on_render: bool,
        render_calls: u32,
        recover_attempts: u32,
    }

    impl AlwaysRecoversButDiesOnFirstRenderBackend {
        fn new() -> Self {
            Self {
                lost: true,
                died_on_render: false,
                render_calls: 0,
                recover_attempts: 0,
            }
        }
    }

    impl RasterBackend for AlwaysRecoversButDiesOnFirstRenderBackend {
        fn render_scene(&mut self, _scene: &flui_layer::Scene) -> Result<bool, EngineError> {
            self.render_calls += 1;
            if !self.died_on_render {
                self.died_on_render = true;
                // Dies again mid-frame, right after the pre-frame recovery
                // (below) just cleared `lost`.
                self.lost = true;
            }
            Ok(true)
        }
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn is_device_lost(&self) -> bool {
            self.lost
        }
        fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
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

    impl DeviceRecovery for AlwaysRecoversButDiesOnFirstRenderBackend {
        fn try_recover_device(&mut self) -> Result<(), EngineError> {
            self.recover_attempts += 1;
            self.lost = false;
            Ok(())
        }
    }

    /// Pins the review's finding #2: gating the post-frame recovery check
    /// on "was the device lost BEFORE this call" (`!was_lost_pre_frame`, the
    /// earlier shape) silently misses a device that recovers successfully
    /// pre-frame and then dies AGAIN during the very `render_frame_entered`
    /// call that follows — `was_lost_pre_frame` reads `true` going in, so
    /// that gate would skip the post-frame check entirely, and the fresh
    /// mid-frame loss would never be attempted or armed at all. Gating on
    /// `next_attempt_at.is_none()` instead correctly re-checks, because a
    /// SUCCESSFUL pre-frame attempt leaves no deadline armed.
    #[test]
    fn a_pre_frame_recovery_success_does_not_block_a_fresh_mid_frame_loss() {
        let realm = mount_root();
        let mut backend = AlwaysRecoversButDiesOnFirstRenderBackend::new();
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert_eq!(
            backend.recover_attempts, 2,
            "the device recovered pre-frame, died again mid-frame, and must be attempted \
             a SECOND time in the same call"
        );
        assert_eq!(
            backend.render_calls, 1,
            "the frame still rendered exactly once"
        );
        assert!(
            !backend.lost,
            "the second, post-frame recovery attempt succeeded"
        );
        assert!(
            outcome.presented,
            "the frame rendered before the second loss reaches present()"
        );
        assert!(
            !outcome.just_failed,
            "the second attempt succeeded too, so nothing failed this call"
        );
    }

    #[test]
    fn a_mid_frame_loss_with_a_failing_recovery_backs_off() {
        let realm = mount_root();
        let mut backend = ScriptedDeviceBackend {
            lose_on_render: true,
            recover_outcome: Some(Err(EngineError::DeviceLost)),
            recover_clears_lost: false,
            ..ScriptedDeviceBackend::healthy()
        };
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert_eq!(
            backend.render_calls, 1,
            "the frame rendered before the loss landed"
        );
        assert!(
            outcome.presented,
            "the frame that rendered still reaches present()"
        );
        assert_eq!(
            backend.recover_attempts, 1,
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
            realm.needs_redraw(),
            "the post-render recovery wake must survive render_frame_entered's own \
             mark_rendered(), so the recovered renderer renders again on a quiescent \
             loop"
        );
    }

    /// The mid-frame counterpart to
    /// `a_pre_frame_loss_with_a_successful_recovery_renders_the_same_frame_and_arms_nothing`:
    /// a device that dies DURING `render_scene` but recovers immediately
    /// afterward must arm nothing either — the frame that just rendered
    /// already presented (or not) on its own merits, and there is no
    /// retry to arm for a device that is healthy again.
    #[test]
    fn a_mid_frame_loss_with_a_successful_recovery_arms_nothing() {
        let realm = mount_root();
        let mut backend = ScriptedDeviceBackend {
            lose_on_render: true,
            ..ScriptedDeviceBackend::healthy()
        };
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert_eq!(
            backend.render_calls, 1,
            "the frame rendered before the loss landed"
        );
        assert!(
            outcome.presented,
            "the frame that rendered still reaches present()"
        );
        assert_eq!(
            backend.recover_attempts, 1,
            "the mid-frame loss is recovered after the render"
        );
        assert!(
            !backend.lost,
            "the scripted successful recovery cleared the lost flag"
        );
        assert!(
            !outcome.just_failed,
            "a SUCCESSFUL post-render recovery must not report a failure"
        );
        assert!(
            outcome.next_attempt_at.is_none(),
            "a successful post-render recovery leaves no pending deadline"
        );
        assert!(
            !realm.needs_redraw(),
            "a presented frame followed by a successful recovery must leave no spurious \
             wake armed"
        );
    }

    /// The 🔴 fix, pinned directly against the observable the deleted
    /// pre-frame `return false` used to kill: a gesture-arena long-press
    /// deadline. `render_frame_entered`'s very first two calls
    /// (`drain_deferred_arena_resolutions`, `flush_pending_moves`) — and
    /// `draw_frame_entered`'s own Vsync tick right after — must all still
    /// run while the device stays lost and every recovery attempt fails,
    /// because they are the ONLY thing that ever resolves a timed-out
    /// gesture, flushes a coalesced pointer move, or ticks an animation.
    /// Before the fix, a still-lost device after a failed pre-frame
    /// recovery attempt returned `false` immediately, and this deadline
    /// would never resolve at all.
    #[test]
    fn a_pre_frame_device_loss_still_resolves_a_pending_gesture_arena_deadline() {
        use flui_interaction::{
            GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
        };

        let realm = mount_root();
        realm.mark_rendered();
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let arena = realm.gestures().arena().clone();
        let long_press_fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_for_callback = std::sync::Arc::clone(&long_press_fired);
        let recognizer = LongPressGestureRecognizer::with_settings(
            arena,
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(1)),
        )
        .with_on_long_press(move || {
            fired_for_callback.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let pointer = PointerId::new(3).expect("nonzero pointer id");
        recognizer.add_pointer(
            pointer,
            flui_types::Offset::new(
                flui_types::geometry::px(10.0),
                flui_types::geometry::px(10.0),
            ),
        );
        assert!(
            realm.gestures().has_pending_deadlines(),
            "precondition: the long-press deadline is actually armed"
        );
        std::thread::sleep(Duration::from_millis(5));

        // A permanently lost device whose recovery never succeeds — the
        // worst case for the deleted early return, and the one production
        // scenario (driver still mid-reset) this fix must keep advancing
        // through.
        let mut backend = ScriptedDeviceBackend {
            lost: true,
            scene_outcome: Some(Err(EngineError::DeviceLost)),
            recover_outcome: Some(Err(EngineError::DeviceLost)),
            recover_clears_lost: false,
            ..ScriptedDeviceBackend::healthy()
        };

        let _ = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);

        assert!(
            long_press_fired.load(std::sync::atomic::Ordering::SeqCst),
            "a pre-frame device loss with a failing recovery must still resolve a due \
             gesture-arena deadline — render_frame_entered's non-GPU work must never be \
             skipped just because the device stayed lost"
        );
    }

    /// Direct reproduction of the review's self-extinguishing-retry finding:
    /// with `wake_frame()` raised INSIDE the recovery attempt (BEFORE
    /// `render_frame_entered` runs, the shape this test would fail
    /// against), `render_frame_entered`'s own tail (`mark_rendered()`,
    /// since a quiescent tree with nothing new to paint produces `Idle`,
    /// not `Errored`) silently clobbers it one hop later. A two-frame drive
    /// does not catch this: the FIRST frame still has the mount's own
    /// pending paint, reaches `render_scene`, and `ui_realm`'s OWN
    /// `DeviceLost` arm independently arms `needs_redraw` too (`retry_needed
    /// = true` there, from this same issue's `ui_realm.rs` fix) — the bug
    /// only shows up once that leftover demand is exhausted, on the frame
    /// AFTER, which is exactly why this drives (and checks) three.
    #[test]
    fn needs_redraw_stays_armed_across_three_consecutive_frames_against_a_permanently_dead_device()
    {
        let realm = mount_root();
        let backoff = DeviceRecoveryBackoff::new();
        let mut now = Instant::now();

        for frame in 1..=3u32 {
            let mut backend = ScriptedDeviceBackend {
                lost: true,
                scene_outcome: Some(Err(EngineError::DeviceLost)),
                recover_outcome: Some(Err(EngineError::DeviceLost)),
                recover_clears_lost: false,
                ..ScriptedDeviceBackend::healthy()
            };
            let _ = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
            assert!(
                realm.needs_redraw(),
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

    /// The test every prior round's test suite was missing: the three
    /// tests above (and every other test in this module) call
    /// `render_frame_with_device_recovery` DIRECTLY, so none of them can
    /// see what happens at the layer in FRONT of it — the desktop closure's
    /// own `dirty` predicate and `wake_action` match, the exact place a
    /// wake-deadline source that never reaches `dirty` returns before this
    /// function is ever called at all. This test drives that outer gate,
    /// verbatim (not a simplification), across enough simulated wakes to
    /// include BOTH failure-mode shapes a real run produces: the immediate
    /// platform poke `wake_frame()` queues on every failure (arrives before
    /// ANY real time passes — reproduced here as `now` staying put, not
    /// advancing), and the deadline-actuated wake the wake-deadline hook
    /// produces once `DeviceRecoveryBackoff`'s own armed instant is
    /// actually due (reproduced as `now` jumping straight to
    /// `next_attempt_at()`). Against the code before this fix (`dirty`
    /// missing the `next_attempt_at().is_some()` term), the SECOND
    /// simulated wake — the immediate poke, arriving before the deadline —
    /// finds nothing else dirty, is silently deferred, and
    /// `render_frame_entered`'s own tail clears `needs_redraw`; the THIRD
    /// wake (the deadline itself, correctly actuated) then reads
    /// `dirty == false` and returns before `render_frame_with_device_
    /// recovery` is even called — a permanently dead device is attempted
    /// exactly once, ever, and the closure never runs again.
    #[test]
    fn the_real_closure_gate_keeps_retrying_across_the_immediate_poke_and_the_deadline_wake() {
        let realm = mount_root();
        let backoff = DeviceRecoveryBackoff::new();
        let mut now = Instant::now();

        fn permanently_dead_backend() -> ScriptedDeviceBackend {
            ScriptedDeviceBackend {
                lost: true,
                scene_outcome: Some(Err(EngineError::DeviceLost)),
                recover_outcome: Some(Err(EngineError::DeviceLost)),
                recover_clears_lost: false,
                ..ScriptedDeviceBackend::healthy()
            }
        }

        // Wake 1: the FIRST detection of the loss. What legitimately
        // triggers it (a mid-frame loss via `ui_realm`'s own `retry_needed`
        // arm, or the realm's very first pending paint) is out of scope
        // for this probe, which exists to check everything AFTER it —
        // called directly, bypassing the closure's own gate, the same way
        // every wake in production reaches this function only once
        // `wake_action` has already decided `Render`.
        let mut backend = permanently_dead_backend();
        let seed = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
        assert!(
            seed.just_failed,
            "precondition: wake 1 is a genuine failure"
        );
        let mut total_attempts = 1u32;
        let mut poke_pending = true;
        // Measured, not argued: the busy-spin the winit backend's own
        // `about_to_wait` module doc names (`platforms/winit/platform.rs`'s
        // `WaitUntil(past)` failure mode) is exactly a hook that keeps
        // answering the SAME stale deadline forever. Collecting every
        // deadline the backoff arms across real attempts and asserting
        // strict growth is the direct check that this hook is never one of
        // those stale sources: each real attempt computes a FRESH deadline
        // from the `now` it actually ran at, never reusing an old one.
        let mut armed_deadlines = vec![seed.next_attempt_at.expect("wake 1 armed a deadline")];

        for wake in 2..=12u32 {
            // The PRODUCTION `frame_is_dirty` call, not a local
            // reimplementation — `inbox_redraw` is always `false` in this
            // scenario (orthogonal to the property under test), the other
            // three arguments read live off `realm`/`backoff`. Round 6's own
            // finding: this test used to recompute the boolean inline, which
            // meant reverting the real closure's `next_attempt_at().is_some()`
            // term left this assertion green — it was checking its own copy
            // of the old logic, not the line the fix actually changed.
            let dirty = frame_is_dirty(
                false, // inbox_redraw: never true in this scenario
                realm.needs_redraw(),
                realm.has_pending_work(),
                backoff.next_attempt_at(),
            );
            assert!(
                dirty,
                "wake {wake}: the real closure gate went Skip and returned before even \
                 checking device recovery -- the retry chain died here (total_attempts so \
                 far: {total_attempts})"
            );

            // The wake that gets here is either the immediate platform
            // poke `wake_frame()` queued on the previous wake's failure
            // (arrives before any real time passes), or -- once that
            // poke's own attempt was deferred (too early) -- the
            // deadline-actuated wake the wake-deadline hook produces
            // exactly at the armed instant.
            now = if poke_pending {
                now
            } else {
                backoff.next_attempt_at().unwrap_or_else(|| {
                    panic!(
                        "wake {wake}: dirty was true with no pending poke, so the only \
                         remaining dirty source must be an armed deadline -- but none is \
                         armed"
                    )
                })
            };

            let mut backend = permanently_dead_backend();
            let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
            if backend.recover_attempts == 1 {
                total_attempts += 1;
                armed_deadlines.push(
                    outcome
                        .next_attempt_at
                        .expect("a real failed attempt always arms a fresh deadline"),
                );
            }
            poke_pending = outcome.just_failed;
        }

        assert!(
            total_attempts >= 4,
            "a persistently failing device must be retried repeatedly across the \
             simulated idle window (immediate poke, deferred, deadline wake, repeat), not \
             collapse to a single attempt -- total_attempts={total_attempts}"
        );
        assert!(
            armed_deadlines.windows(2).all(|pair| pair[1] > pair[0]),
            "every real attempt must arm a STRICTLY LATER deadline than the one before it \
             -- a non-growing or repeated deadline is exactly the `WaitUntil(past)` shape \
             the winit backend's own `about_to_wait` module doc names as a busy-spin \
             (deadline never refreshed -> `about_to_wait` -> `WaitUntil(past)` -> \
             `new_events` -> `request_redraw` -> repeat at 100% CPU): {armed_deadlines:?}"
        );
    }

    /// The Android counterpart of
    /// `the_real_closure_gate_keeps_retrying_across_the_immediate_poke_and_the_deadline_wake`
    /// above, driving the same `frame_is_dirty` call `bootstrap_android`'s
    /// closure makes (`has_pending` is just that closure's own local name
    /// for `realm.has_pending_work()`).
    ///
    /// **What this pins, precisely — and what it does not.** This test
    /// exercises only the `flui-app`-side gate: `frame_is_dirty` and
    /// `DeviceRecoveryBackoff` are plain functions/types this crate owns and
    /// can call directly. It does NOT drive `flui-platform`'s
    /// `AndroidPlatform::run` loop — `is_deadline_due`, the `resumed` state
    /// machine, `should_render`, or the 0ms/16ms `timeout` switch — because
    /// that loop needs a live `AndroidApp`, which has no test double on any
    /// host (see `is_deadline_due`'s own module for the parts of that state
    /// machine that ARE unit-tested there, in isolation, without `run`
    /// itself). The one adjustment made here to acknowledge that gap: `now`
    /// lands `NO_PRESENT_FALLBACK_PACE` (16ms) past each armed deadline
    /// rather than exactly on it, approximating that `is_deadline_due` is
    /// polled once per ~16ms idle tick rather than actuated exactly at the
    /// instant like desktop's `ControlFlow::WaitUntil`. Do not read this
    /// test's `total_attempts` as a measurement of the native loop's actual
    /// retry cadence — it measures the shared `flui-app` gate/backoff
    /// invariant under a coarser deadline-catch approximation, nothing more.
    #[test]
    fn the_real_closure_gate_keeps_retrying_on_android_across_the_immediate_poke_and_the_16ms_poll()
    {
        let realm = mount_root();
        let backoff = DeviceRecoveryBackoff::new();
        let mut now = Instant::now();

        fn permanently_dead_backend() -> ScriptedDeviceBackend {
            ScriptedDeviceBackend {
                lost: true,
                scene_outcome: Some(Err(EngineError::DeviceLost)),
                recover_outcome: Some(Err(EngineError::DeviceLost)),
                recover_clears_lost: false,
                ..ScriptedDeviceBackend::healthy()
            }
        }

        let mut backend = permanently_dead_backend();
        let seed = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
        assert!(
            seed.just_failed,
            "precondition: wake 1 is a genuine failure"
        );
        let mut total_attempts = 1u32;
        let mut poke_pending = true;

        for wake in 2..=12u32 {
            let has_pending = realm.has_pending_work();
            // The PRODUCTION `frame_is_dirty` call — see this test's own doc
            // for why reimplementing it inline here would silently stop
            // pinning the fix.
            let dirty = frame_is_dirty(
                false, // inbox_redraw: never true in this scenario
                realm.needs_redraw(),
                has_pending,
                backoff.next_attempt_at(),
            );
            assert!(
                dirty,
                "wake {wake}: Android's closure gate went Skip and returned before even \
                 checking device recovery -- the retry chain died here (total_attempts so \
                 far: {total_attempts})"
            );

            now = if poke_pending {
                now
            } else {
                // Android's own `is_deadline_due` granularity: caught on
                // the next ~16ms poll, not exactly at the deadline.
                backoff.next_attempt_at().unwrap_or_else(|| {
                    panic!("wake {wake}: dirty was true with no pending poke and no armed deadline")
                }) + super::NO_PRESENT_FALLBACK_PACE
            };

            let mut backend = permanently_dead_backend();
            let outcome = render_frame_with_device_recovery(&realm, &mut backend, &backoff, now);
            if backend.recover_attempts == 1 {
                total_attempts += 1;
            }
            poke_pending = outcome.just_failed;
        }

        assert!(
            total_attempts >= 4,
            "a persistently failing device must be retried repeatedly on Android too, not \
             collapse to a single attempt -- total_attempts={total_attempts}"
        );
    }

    /// A persistently failing recovery must be attempted a BOUNDED number
    /// of times over a fixed window, not once per delivered platform frame
    /// — the same property issue #556's `no_present_fallback_bounds_
    /// repeating_no_present_wakes` measures for the no-present pace,
    /// applied here to [`DeviceRecoveryBackoff`] directly. Driven entirely
    /// in VIRTUAL time (`now` advanced by hand to each returned deadline,
    /// never `std::thread::sleep`): the backoff's own API takes `now`
    /// explicitly for exactly this reason, so a much longer window is
    /// provable in microseconds of real wall-clock time instead of costing
    /// the window for real — this test used to be the slowest in the
    /// suite at ~1s; it no longer sleeps at all.
    #[test]
    fn device_recovery_backoff_bounds_a_persistently_failing_retry_loop() {
        let backoff = DeviceRecoveryBackoff::new();
        let mut now = Instant::now();
        let window = Duration::from_mins(10);
        let window_end = now + window;
        let mut attempts = 0u32;

        while now < window_end {
            attempts += 1;
            now = backoff.record_failure(&EngineError::DeviceLost, now);
        }

        let unbacked_off_attempts =
            u32::try_from(window.as_millis() / DeviceRecoveryBackoff::BASE.as_millis())
                .unwrap_or(u32::MAX);
        assert!(
            attempts < 700,
            "the backoff failed to bound the retry loop: {attempts} attempts over a \
             simulated {window:?} (an un-backed-off fixed cadence would rack up roughly \
             {unbacked_off_attempts} in the same window — once capped at one second per \
             attempt, {window:?} allows at most about 600)",
        );
        assert!(
            attempts >= 2,
            "sanity: the loop must actually retry more than once, not just give up after \
             the first failure (attempts={attempts})"
        );
    }

    #[test]
    fn device_recovery_backoff_resets_to_the_base_interval_after_a_success() {
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();

        let first = backoff.record_failure(&EngineError::DeviceLost, now);
        let second = backoff.record_failure(&EngineError::DeviceLost, now);
        assert_eq!(first - now, DeviceRecoveryBackoff::BASE);
        assert!(
            second - now > first - now,
            "the backoff must grow across consecutive failures with no success in between"
        );

        backoff.record_success();
        let after_reset = backoff.record_failure(&EngineError::DeviceLost, now);
        assert_eq!(
            after_reset - now,
            DeviceRecoveryBackoff::BASE,
            "a success must reset the backoff to its base interval, not continue growing \
             from where it left off"
        );
    }

    #[test]
    fn device_recovery_backoff_caps_at_the_ceiling_and_never_gives_up() {
        let backoff = DeviceRecoveryBackoff::new();
        let now = Instant::now();
        let mut last = now;

        // Comfortably past the point the shift itself saturates.
        for _ in 0..20u32 {
            last = backoff.record_failure(&EngineError::DeviceLost, now);
        }

        assert_eq!(
            last - now,
            DeviceRecoveryBackoff::CAP,
            "the interval must stop growing at the cap instead of overflowing or \
             continuing to double"
        );
        // "Never gives up" is an absence: there is no attempt-count field
        // anywhere on this type that could refuse a 21st call — the type
        // itself has no such state to check, which is the point.
    }
}
