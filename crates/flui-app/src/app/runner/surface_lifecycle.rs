// ============================================================================
// Surface release/rebuild (the native window going away and coming back)
// ============================================================================
//
// The sync surface-lifecycle seam a runner needs when the native window
// behind its presentation dies and is later replaced. `flui-platform` owns
// the event loop that observes this and reports it as a bare `bool` through
// `PlatformWindow::on_surface_status_change`; this module is where a runner
// turns that bool into an act on its renderer.
//
// It follows `device_recovery.rs`: a `pub(super)` trait narrows the concrete
// `flui_engine::Renderer` to the verbs a runner needs, so the decision
// is host-testable against a scripted backend while the `Renderer`-side
// mechanics stay type-checked only (they need a GPU). The module is
// registered unconditionally — see `mod.rs` — so every host gate compiles it.

use flui_engine::EngineError;

/// The sync surface release/rebuild seam a runner needs from its renderer.
///
/// [`flui_engine::RasterBackend`] carries no such verb: releasing a surface is
/// a whole-object act with no frame to attach it to (see
/// `wgpu/surface_lease.rs`'s "A lease may hold no surface at all"), so this
/// seam narrows the concrete renderer instead of widening the public trait —
/// the shape [`super::device_recovery::DeviceRecovery`] already uses. The
/// production impl forwards to `Renderer`'s own inherent methods; test fakes
/// script both verbs.
///
/// The two verbs are deliberately asymmetric. [`Self::release_surface`] asks
/// for a post-state that can already hold, so a repeated request is harmless;
/// [`Self::recreate_surface`] asks about the present and never consults what
/// the renderer holds. [`ensure_surface`] states why that asymmetry is what
/// makes the whole seam safe to drive from an event that can be missed.
pub(super) trait SurfaceLifecycle {
    /// Drop the surface this renderer presents through, keeping the retained
    /// window target and the device so [`Self::recreate_surface`] can build
    /// the next one.
    ///
    /// Idempotent: a renderer that already holds no surface changes nothing.
    /// The released state lives in the surface lease, whose `release`
    /// short-circuits on it — the caller holds no state to dedupe with, and
    /// must not add any (see [`ensure_surface`]).
    ///
    /// Dropping a *configured* `wgpu::Surface` is not free: it reaches
    /// `vkDeviceWaitIdle`, an unbounded wait on whichever thread makes the
    /// call. That is why this verb must not wait on a raster lane, probe the
    /// window target or submit anything: the surface's own drop is the only
    /// cost it is allowed to pay, and every other cost on this path would
    /// stack on top of it.
    ///
    /// # That wait runs under the caller's held lane guard, by design
    ///
    /// The only production caller takes the raster lane's **blocking** guard
    /// and calls this verb inside it, so the unbounded device-idle wait above
    /// is paid with the lane held, on the thread that delivers the
    /// availability callback. That is accepted rather than avoided, and the
    /// reason is the ordering the request carries: the surface has to be gone
    /// *before the callback returns*, because the callback is the last moment
    /// the native handle behind it is valid — on Android the handle is cleared
    /// by the command the other side applies once this callback is done. A
    /// skipped release (which is what a non-blocking lock would produce under
    /// contention) leaves a configured surface built from a dead handle, the
    /// class this whole seam exists to remove, so the guard is held
    /// deliberately and contention is not an acceptable outcome.
    ///
    /// **Unverified:** the wait's length is the driver's, and no Android
    /// device is available here to measure it. The contingent risk is
    /// Android's input-dispatch watchdog, which polices the thread that
    /// requested this transition: a wait longer than its budget is an ANR
    /// rather than a skipped frame.
    fn release_surface(&mut self);

    /// Build and configure a fresh surface from the retained window target,
    /// replacing whatever the renderer held.
    ///
    /// `Err` means nothing was rebuilt: the renderer is left holding no
    /// surface, so its next frame is a skipped one
    /// ([`flui_engine::RasterBackend::render_scene`]'s third `Ok(false)`
    /// cause) rather than a frame presented into a surface whose native
    /// window is gone. The caller owns the response to that error; it is
    /// carried out rather than logged in here.
    fn recreate_surface(&mut self) -> Result<(), EngineError>;
}

#[cfg(not(target_arch = "wasm32"))]
impl SurfaceLifecycle for flui_engine::Renderer {
    fn release_surface(&mut self) {
        // `Renderer::release_surface` is the inherent method of the same
        // name; the qualified path is what keeps this from resolving back to
        // the trait method it implements.
        flui_engine::Renderer::release_surface(self);
    }

    fn recreate_surface(&mut self) -> Result<(), EngineError> {
        flui_engine::Renderer::recreate_surface(self)
    }
}

/// What [`ensure_surface`] did, and what the caller still owes for it.
///
/// `#[must_use]`: [`Self::Recreated`] carries an obligation, and a caller
/// that discards it leaves a resumed presentation presenting into a surface
/// whose identity nothing has refreshed.
#[must_use]
#[derive(Debug)]
// Only the mobile runners (Android, iOS) and this module's own tests drive
// the seam, so a host build without tests (and every other target) sees it as
// unused.
#[cfg_attr(
    not(any(test, target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests -- see module doc"
    )
)]
pub(super) enum SurfaceLifecycleOutcome {
    /// The surface was released, and nothing else is owed. A released
    /// renderer skips its frames while its authoritative size keeps
    /// advancing, so a resize that arrives while released is still honoured.
    Released,
    /// A fresh surface is configured and the renderer can present again.
    ///
    /// The caller owes two more things, neither of which this seam can do
    /// for it: a surface-generation mint on its raster lane, so no stale
    /// in-flight work stays addressed to the destroyed surface (the same act
    /// device-loss recovery performs), and a full repaint, because a fresh
    /// surface's contents are undefined while the damage tracker is
    /// incremental.
    Recreated,
    /// A recreation was requested and failed, carrying the renderer's own
    /// error. The presentation is left released.
    ///
    /// The caller owns the response, and the shape of it lives in
    /// [`settle_surface_availability`], which both mobile runners call: the
    /// carried error is classified once by [`SurfaceSettlement::classify`],
    /// logged through [`report_surface_settlement`], and never minted. One
    /// classification is downgraded rather than warned about:
    /// [`EngineError::SurfaceTargetUnavailable`] is the *expected* answer to
    /// the acquire half on a backend where the availability signal arrives
    /// before any window exists, so it is logged at a level that does not
    /// make a working path noisy. Every other error is the `warn`.
    ///
    /// # A failed recreation does not heal itself through this seam
    ///
    /// The seam is stateless, so the next request is answered correctly, but
    /// a request has to *arrive*, and the mobile backends' only emitters are
    /// the lifecycle events that produce the `false`/`true` pair. What
    /// statelessness buys is that a **missed signal** cannot strand the
    /// presentation: the next `true` re-asks, and asks about the present
    /// rather than consulting what is held. It buys nothing for a failure that
    /// has already been observed and reported, because there may be no next
    /// signal to re-ask on.
    ///
    /// That re-ask is owned by [`SurfaceRecreationRetry`], not by this seam:
    /// a genuine failure arms a deadline-paced retry that the runner's frame
    /// closure drives through [`retry_surface_recreation`], exactly as the
    /// sibling device-loss path (`super::device_recovery`) retries a lost
    /// device under its own backoff.
    Failed(EngineError),
}

/// Bring the renderer's surface into line with `should_hold_surface`:
/// `false` releases it, `true` rebuilds it unconditionally.
///
/// **Stateless by construction.** `true` asks for a surface valid for the
/// window handle available *now*, so it recreates even when one is already
/// held; `false` asks for a post-state that can already hold, so a repeated
/// `false` reaches the renderer again and is a no-op there
/// ([`SurfaceLifecycle::release_surface`]). Neither direction consults what
/// came before. That is what removes the "a missed release strands a surface
/// built from a dead window for the rest of the process's life" failure
/// mode: the next `true` re-asks instead of being mistaken for a redundant
/// one, so correctness does not depend on having received every `false`.
///
/// Nothing is logged here. The caller owns the log line, because only it
/// knows which signal produced the request and at what frequency; see the
/// the mobile runners' registration for the levels.
#[cfg_attr(
    not(any(test, target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests -- see module doc"
    )
)]
pub(super) fn ensure_surface<L: SurfaceLifecycle>(
    backend: &mut L,
    should_hold_surface: bool,
) -> SurfaceLifecycleOutcome {
    if should_hold_surface {
        match backend.recreate_surface() {
            Ok(()) => SurfaceLifecycleOutcome::Recreated,
            Err(source) => SurfaceLifecycleOutcome::Failed(source),
        }
    } else {
        backend.release_surface();
        SurfaceLifecycleOutcome::Released
    }
}

/// The label [`SurfaceRecreationRetry`]'s backoff logs under.
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests"
    )
)]
pub(super) const SURFACE_RECREATION_LABEL: &str = "wgpu surface recreation";

/// Which terminal outcome one availability callback produced, from the
/// retry's point of view — the distinction that decides whether a retry is
/// owed.
///
/// A release and a successful recreation both clear the retry. A *genuine*
/// failure arms it; the probe's own [`EngineError::SurfaceTargetUnavailable`]
/// ("the target has no handle right now") does NOT, because that is the
/// designed answer on a backend where the availability signal arrives before
/// any window exists, and the next signal recreates correctly.
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests -- see module doc"
    )
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SurfaceSettlement {
    /// The availability signal asked for a release; no retry is owed.
    Released,
    /// A fresh surface is configured; no retry is owed.
    Recreated,
    /// The rebuild failed with `SurfaceTargetUnavailable`: the window is not
    /// there yet, which is expected, not late. No retry is owed.
    TargetUnavailable,
    /// A genuine rebuild failure: a retry is owed and now deadline-paced.
    Failed,
}

impl SurfaceSettlement {
    /// Classify the terminal outcome of one availability callback.
    ///
    /// This is the ONE place the expected-vs-genuine failure distinction is
    /// made, so both mobile runners classify identically — the drift class
    /// this repository's "one fact, one place" rule exists to prevent.
    ///
    /// The platform's signal does not appear here: the outcome already
    /// carries it (`ensure_surface` returns `Released` only for a `false`
    /// signal and `Recreated`/`Failed` only for a `true` one), so the
    /// classification is a function of the outcome alone.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn classify(outcome: &SurfaceLifecycleOutcome) -> SurfaceSettlement {
        match outcome {
            SurfaceLifecycleOutcome::Released => SurfaceSettlement::Released,
            SurfaceLifecycleOutcome::Recreated => SurfaceSettlement::Recreated,
            SurfaceLifecycleOutcome::Failed(source) => {
                if matches!(source, EngineError::SurfaceTargetUnavailable { .. }) {
                    SurfaceSettlement::TargetUnavailable
                } else {
                    SurfaceSettlement::Failed
                }
            }
        }
    }

    /// The level a settlement's log line earns, or `None` for the outcomes
    /// that log nothing here (the engine already logs a release, and the lane
    /// logs a re-mint).
    ///
    /// The expected [`Self::TargetUnavailable`] is `TRACE`, not `WARN` or
    /// `DEBUG`: it happens on every cycle of a backend whose availability
    /// signal can arrive before a window exists, and a line at `debug` there
    /// is guaranteed noise on a path working as designed. A genuine
    /// [`Self::Failed`] is a `WARN` naming the retry that now polls it.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn report_level(self) -> Option<tracing::Level> {
        match self {
            SurfaceSettlement::Released | SurfaceSettlement::Recreated => None,
            SurfaceSettlement::TargetUnavailable => Some(tracing::Level::TRACE),
            SurfaceSettlement::Failed => Some(tracing::Level::WARN),
        }
    }
}

/// Drives automatic retry of a failed surface recreation.
///
/// The seam in this module ([`ensure_surface`]) is stateless and correct, and
/// it deliberately declines to retry on its own — see
/// [`SurfaceLifecycleOutcome::Failed`]'s doc, which names the residual: nothing
/// re-asks until another availability signal arrives, and the only emitters on
/// the mobile backends are the lifecycle events that produce the `false`/`true`
/// pair. On a window that stays available, that means a genuine rebuild failure
/// leaves the presentation released and every frame after it skipped, until
/// some *unrelated* lifecycle event happens to re-emit `true`.
///
/// This type closes that residual for the one failure class that can heal on
/// its own — a rebuild that failed for a reason other than the probe's own
/// "no window yet" answer. It reuses the exact deadline-paced retry device-loss
/// recovery already runs ([`super::retry_backoff::RetryBackoff`]): the frame
/// closure consults [`Self::next_attempt_at`] in its `dirty` predicate, the
/// platform's wake-deadline hook carries the deadline, and
/// [`Self::attempt_if_due`] makes one gated attempt per due deadline.
///
/// **Deliberately not armed for the expected failure.** The probe's
/// [`EngineError::SurfaceTargetUnavailable`] ("the target has no handle right
/// now") is the *designed* answer to the acquire half on a backend where the
/// availability signal arrives before any window exists; the next `true`
/// recreates correctly and no retry is owed. Arming a retry for it would poll a
/// condition that is not late, just absent — [`SurfaceSettlement::classify`] is
/// where that classification lives (the platform callback only sees the bool).
///
/// **Gated on the last availability signal.** A retry must only run while the
/// platform's most recent signal said a surface is *expected*; a later `false`
/// (backgrounded, window gone) means a retry would be building a surface the
/// platform just said not to — and the next `true` will recreate correctly
/// anyway. So the retry records the signal, not only the failure.
// Only the mobile runners (Android, iOS) construct this, so a host build
// without tests (and every other target) sees it as unused -- same gate as
// `SurfaceLifecycleOutcome` above.
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests -- see module doc"
    )
)]
pub(super) struct SurfaceRecreationRetry {
    backoff: super::retry_backoff::RetryBackoff,
    /// The most recent availability signal. `true` on Android's `InitWindow`/
    /// `Resume` and iOS's scene connect, `false` on `TerminateWindow`/`Pause`
    /// and scene disconnect. A retry is only attempted while this is `true`.
    expects_surface: std::sync::atomic::AtomicBool,
}

impl SurfaceRecreationRetry {
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn new() -> Self {
        Self {
            backoff: super::retry_backoff::RetryBackoff::new(SURFACE_RECREATION_LABEL),
            expects_surface: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Record one availability callback: the platform's signal (`true` =
    /// surface expected) and the terminal outcome it produced.
    ///
    /// - A `false` signal clears any armed retry: a later `true` recreates
    ///   unconditionally, so a retry would be redundant.
    /// - `Recreated`, `Released`, or `TargetUnavailable` clear the retry —
    ///   nothing is owed in any of those.
    /// - A genuine `Failed` while a surface is still expected arms the
    ///   deadline-paced retry.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn note_availability(
        &self,
        expects_surface: bool,
        settlement: SurfaceSettlement,
        error: Option<&EngineError>,
        now: web_time::Instant,
    ) {
        self.expects_surface
            .store(expects_surface, std::sync::atomic::Ordering::Release);
        match settlement {
            SurfaceSettlement::Failed if expects_surface => {
                if let Some(error) = error {
                    self.backoff.record_failure(error, now);
                }
            }
            // A release, a successful recreation, an expected
            // target-unavailable, or a failure while the platform does NOT
            // expect a surface: no retry is owed.
            _ => self.backoff.record_success(),
        }
    }

    /// The earliest instant the next retry attempt is allowed, if one is owed.
    /// `None` when nothing is owed (never failed, released, recovered, or the
    /// platform last said no surface is expected). Read by the frame closure's
    /// `dirty` predicate and the wake-deadline hook — the same two consumers
    /// device recovery has.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn next_attempt_at(&self) -> Option<web_time::Instant> {
        if self.expects_surface() {
            self.backoff.next_attempt_at()
        } else {
            None
        }
    }

    /// Whether the platform's last availability signal said a surface is
    /// expected. The frame closure reads this to decide whether a released
    /// surface is one a retry may build.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn expects_surface(&self) -> bool {
        self.expects_surface
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Make one retry attempt when the armed deadline has elapsed; otherwise
    /// return `None` without touching the backend. Refuses outright when the
    /// platform does not expect a surface (see this type's own doc).
    ///
    /// On success the retry is cleared and `Some(Recreated)` is returned — the
    /// caller owes a surface-generation mint and a full repaint, exactly as it
    /// owes for an availability-driven [`SurfaceLifecycleOutcome::Recreated`].
    /// On a fresh failure the next deadline is armed and `Some(Failed)` is
    /// returned. `None` means "nothing owed, not due yet, or no surface is
    /// expected".
    ///
    /// The deadline is a `CHECK`, never a sleep: a retry attempt can rebuild a
    /// surface, so pacing it on this platform's own idle wait is the only
    /// non-blocking option (see [`super::retry_backoff::RetryBackoff`]'s doc).
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "driven by the Android/iOS runners and by this module's tests"
        )
    )]
    pub(super) fn attempt_if_due<L: SurfaceLifecycle>(
        &self,
        backend: &mut L,
        now: web_time::Instant,
    ) -> Option<SurfaceLifecycleOutcome> {
        if !self.expects_surface() {
            return None;
        }
        if let Some(deadline) = self.backoff.next_attempt_at()
            && now < deadline
        {
            return None;
        }
        match backend.recreate_surface() {
            Ok(()) => {
                self.backoff.record_success();
                Some(SurfaceLifecycleOutcome::Recreated)
            }
            Err(source) => {
                self.backoff.record_failure(&source, now);
                Some(SurfaceLifecycleOutcome::Failed(source))
            }
        }
    }
}

/// Settle one platform availability signal against the presentation's raster
/// lane: drive [`ensure_surface`], mint a fresh surface generation on a
/// rebuild, and record the outcome with the retry.
///
/// This is the body of both mobile runners' `on_surface_status_change`
/// callback, pulled out so the two cannot drift: the engine's tracker mark
/// and the generation mint run together under the caller's lane lock (no
/// stale in-flight work stays addressed to the destroyed surface — the same
/// act device-loss recovery performs, through the same mailbox counter), and
/// the failure classification is made exactly once, here, so this callback
/// and the frame-path retry agree about which failure is genuine.
///
/// Two obligations stay with the caller, because they need the lane lock
/// released first: dispatch a full repaint of the realm on
/// [`SurfaceLifecycleOutcome::Recreated`] (the realm half is deferrable and
/// goes through the realm dispatch), and log the outcome through
/// [`report_surface_settlement`].
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests"
    )
)]
pub(super) fn settle_surface_availability<B>(
    lane: &mut crate::app::raster_lane::RasterLane<B>,
    retry: &SurfaceRecreationRetry,
    has_surface: bool,
    now: web_time::Instant,
) -> SurfaceLifecycleOutcome
where
    B: flui_engine::RasterBackend + SurfaceLifecycle,
{
    let outcome = lane.with_backend(|renderer| ensure_surface(renderer, has_surface));
    if matches!(outcome, SurfaceLifecycleOutcome::Recreated) {
        lane.note_surface_recreated();
    }
    let settlement = SurfaceSettlement::classify(&outcome);
    let error = match &outcome {
        SurfaceLifecycleOutcome::Failed(source) => Some(source),
        _ => None,
    };
    retry.note_availability(has_surface, settlement, error, now);
    outcome
}

/// Log the outcome of one availability callback, at the level its
/// classification earns ([`SurfaceSettlement::report_level`]). Called after
/// the caller's lane lock is released.
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests"
    )
)]
pub(super) fn report_surface_settlement(platform: &'static str, outcome: &SurfaceLifecycleOutcome) {
    let SurfaceLifecycleOutcome::Failed(source) = outcome else {
        return;
    };
    match SurfaceSettlement::classify(outcome).report_level() {
        Some(tracing::Level::WARN) => tracing::warn!(
            platform,
            ?source,
            "the wgpu surface could not be rebuilt after the window was reported available; \
             a deadline-paced retry is armed"
        ),
        Some(_) => tracing::trace!(
            platform,
            ?source,
            "no window to rebuild the surface from yet; the next availability signal brings one"
        ),
        None => {}
    }
}

/// Make the frame path's gated retry attempt, if one is owed and due.
///
/// Runs BEFORE the frame, in its own lane-lock scope, so the generation mint
/// happens under the lock and the caller's realm half happens outside it —
/// the same shape [`settle_surface_availability`] uses. Returns `None`
/// without touching the lane when no retry is armed, when the deadline has
/// not elapsed, or when the lane is already held by an outer frame dispatch
/// (logged, like a skipped frame; the deadline stays armed, so the next wake
/// retries). On [`SurfaceLifecycleOutcome::Recreated`] the caller owes the
/// realm a full repaint, exactly as it does after an availability-driven
/// rebuild — and, since the callers run inside the realm's own Frame task,
/// they mark it on the realm directly so the frame that follows is the one
/// that repaints, rather than queueing another task behind themselves.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(
    not(any(target_os = "android", target_os = "ios")),
    expect(
        dead_code,
        reason = "driven by the Android/iOS runners and by this module's tests"
    )
)]
pub(super) fn retry_surface_recreation<B>(
    lane: &parking_lot::Mutex<crate::app::raster_lane::RasterLane<B>>,
    retry: &SurfaceRecreationRetry,
    now: web_time::Instant,
) -> Option<SurfaceLifecycleOutcome>
where
    B: flui_engine::RasterBackend + SurfaceLifecycle,
{
    // Cheap pre-check before the lock: the common case is "nothing armed".
    retry.next_attempt_at()?;
    let Some(mut lane) = lane.try_lock() else {
        tracing::error!(
            "surface-recreation retry skipped: raster lane already held by an outer frame \
             dispatch"
        );
        return None;
    };
    let outcome = lane.with_backend(|renderer| retry.attempt_if_due(renderer, now));
    if matches!(outcome, Some(SurfaceLifecycleOutcome::Recreated)) {
        lane.note_surface_recreated();
    }
    outcome
}

/// The surface release/rebuild contract of [`ensure_surface`] against a
/// scripted backend: a release reaches the renderer and a repeat is not
/// skipped (the seam holds no state to skip it with), an acquire recreates
/// over a surface that is already held rather than short-circuiting, a
/// failed rebuild is tolerated as a typed outcome that leaves the
/// presentation released, and one release-then-acquire cycle records exactly
/// those two calls.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod surface_lifecycle_tests {

    use flui_engine::{EngineError, PresentDisposition, RasterBackend};

    use super::{SurfaceLifecycle, SurfaceLifecycleOutcome, ensure_surface};

    /// One verb the seam asked a scripted backend for, in the order asked.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SurfaceCall {
        Release,
        Recreate,
    }

    /// A scripted [`SurfaceLifecycle`] backend: it records the verb sequence
    /// [`ensure_surface`] produced and models the one post-state the seam's
    /// contract names — whether a surface is currently held — so a test can
    /// assert the released post-state without a GPU.
    ///
    /// The flag stands in for what the real renderer's surface lease holds
    /// (`wgpu/surface_lease.rs`, whose own tests pin the lease's half); it is
    /// kept deliberately dumb, committing a surface on success only, which is
    /// `Renderer::recreate_surface`'s commit-or-nothing discipline.
    struct ScriptedSurfaceBackend {
        calls: Vec<SurfaceCall>,
        /// The id of the surface currently held, if any. A successful
        /// recreation mints a fresh id, so a test can see that a previously
        /// held surface was replaced rather than kept.
        held: Option<u32>,
        next_surface_id: u32,
        /// `recreate_surface`'s scripted outcome (`take`n — `EngineError` is
        /// not `Clone`).
        recreate_outcome: Option<Result<(), EngineError>>,
    }

    impl ScriptedSurfaceBackend {
        /// A backend already holding a surface, whose next recreation
        /// succeeds.
        fn holding_a_surface() -> Self {
            Self {
                calls: Vec::new(),
                held: Some(0),
                next_surface_id: 1,
                recreate_outcome: Some(Ok(())),
            }
        }

        /// A backend holding nothing, whose next recreation fails with
        /// `error`.
        fn whose_recreation_fails(error: EngineError) -> Self {
            Self {
                calls: Vec::new(),
                held: None,
                next_surface_id: 1,
                recreate_outcome: Some(Err(error)),
            }
        }
    }

    /// The raster-lane half of the fake: the lane the runner helpers drive
    /// is generic over [`RasterBackend`], and nothing here renders, so every
    /// verb is a stub.
    impl RasterBackend for ScriptedSurfaceBackend {
        fn render_scene(
            &mut self,
            _scene: &flui_layer::Scene,
        ) -> Result<PresentDisposition, EngineError> {
            Ok(PresentDisposition::Presented)
        }
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn is_device_lost(&self) -> bool {
            false
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

    impl SurfaceLifecycle for ScriptedSurfaceBackend {
        fn release_surface(&mut self) {
            self.calls.push(SurfaceCall::Release);
            self.held = None;
        }

        fn recreate_surface(&mut self) -> Result<(), EngineError> {
            self.calls.push(SurfaceCall::Recreate);
            let outcome = self
                .recreate_outcome
                .take()
                .expect("recreate_surface called more than once in a test");
            if outcome.is_ok() {
                self.next_surface_id += 1;
                self.held = Some(self.next_surface_id);
            }
            outcome
        }
    }

    /// The `true` edge: an acquire asks for a recreation even though a
    /// surface is already held, and on this scripted backend the surface it
    /// ends up holding is a different one. The outcome is `Recreated`, which
    /// is the generation-mint-and-full-repaint obligation the caller must not
    /// discard.
    ///
    /// This pins the *seam*: `ensure_surface` never consults what is held and
    /// never short-circuits. It does not pin the platform. The real Vulkan
    /// backend allows one surface per `ANativeWindow`, which is why
    /// `Renderer::recreate_surface` releases the held surface before creating
    /// the replacement (see that method's doc): a second `true` over a surface
    /// still bound to the same live window then creates cleanly instead of
    /// being aborted inside `wgpu-hal`'s `create_surface_android`. The
    /// scripted backend succeeds unconditionally because the property under
    /// test is the seam's, not the driver's.
    #[test]
    fn an_acquire_request_recreates_over_a_held_surface_and_reports_recreated() {
        let mut backend = ScriptedSurfaceBackend::holding_a_surface();
        let before = backend.held;

        let outcome = ensure_surface(&mut backend, true);

        assert!(
            matches!(outcome, SurfaceLifecycleOutcome::Recreated),
            "a successful recreation reports Recreated, got {outcome:?}"
        );
        assert_eq!(
            backend.calls,
            vec![SurfaceCall::Recreate],
            "an acquire does not probe first and does not release first"
        );
        assert!(backend.held.is_some(), "the renderer holds a surface again");
        assert_ne!(
            backend.held, before,
            "the previously held surface was replaced, not kept"
        );
    }

    /// A recreation that fails is tolerated, not propagated and not retried:
    /// the seam hands the renderer's own error to its caller, which owns the
    /// log line, and leaves the presentation released — no surface is
    /// committed, and no extra release is issued to "clean up" after it.
    #[test]
    fn a_failed_recreation_carries_the_error_and_leaves_the_presentation_released() {
        let mut backend =
            ScriptedSurfaceBackend::whose_recreation_fails(EngineError::SurfaceCreation(Box::new(
                std::io::Error::other("scripted recreation failure"),
            )));

        let outcome = ensure_surface(&mut backend, true);

        let source = match outcome {
            SurfaceLifecycleOutcome::Failed(source) => source,
            other => panic!("a failed recreation reports Failed, got {other:?}"),
        };
        assert!(
            source.to_string().contains("scripted recreation failure"),
            "the carried error is the renderer's own, got: {source}"
        );
        assert_eq!(
            backend.calls,
            vec![SurfaceCall::Recreate],
            "a failure re-asks nothing and adds no release"
        );
        assert_eq!(
            backend.held, None,
            "a failed rebuild commits no surface, so the released state stands"
        );
    }

    // ------------------------------------------------------------------
    // Automatic retry of a failed recreation (`SurfaceRecreationRetry`)
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Classification (`SurfaceSettlement::classify`)
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // The runner helpers over a raster lane
    // ------------------------------------------------------------------
}
