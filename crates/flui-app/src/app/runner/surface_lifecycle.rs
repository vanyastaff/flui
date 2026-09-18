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
// `flui_engine::wgpu::Renderer` to the verbs a runner needs, so the decision
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

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
impl SurfaceLifecycle for flui_engine::wgpu::Renderer {
    fn release_surface(&mut self) {
        // `Renderer::release_surface` is the inherent method of the same
        // name; the qualified path is what keeps this from resolving back to
        // the trait method it implements.
        flui_engine::wgpu::Renderer::release_surface(self);
    }

    fn recreate_surface(&mut self) -> Result<(), EngineError> {
        flui_engine::wgpu::Renderer::recreate_surface(self)
    }
}

/// What [`ensure_surface`] did, and what the caller still owes for it.
///
/// `#[must_use]`: [`Self::Recreated`] carries an obligation, and a caller
/// that discards it leaves a resumed presentation presenting into a surface
/// whose identity nothing has refreshed.
#[must_use]
#[derive(Debug)]
// Only the Android runner and this module's own tests drive the seam, so a
// host build without tests (and every other target) sees it as unused.
#[cfg_attr(
    not(any(test, target_os = "android")),
    expect(
        dead_code,
        reason = "driven by the Android runner and by this module's tests -- see module doc"
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
    /// The caller owns the response, and the shape of it is in
    /// [`ensure_surface`]'s caller (the Android runner's registration): log
    /// the carried error as the `source` of a `warn`, and never mint. One
    /// classification is the caller's to downgrade rather than to warn about:
    /// [`EngineError::SurfaceTargetUnavailable`] is the *expected* answer to
    /// the acquire half on a backend where the availability signal arrives
    /// before any window exists, so it is logged at a level that does not
    /// make a working path noisy. Every other error is the `warn`.
    ///
    /// # A failed recreation is a residual, not a state that heals itself
    ///
    /// Nothing re-asks on its own. The seam is stateless, so the next request
    /// is answered correctly, but a request has to *arrive*, and this
    /// backend's only emitters are the lifecycle events that produce the
    /// `false`/`true` pair. A recreation that fails for a reason other than
    /// the expected `SurfaceTargetUnavailable` therefore leaves the
    /// presentation released, with every frame after it a skipped one, until
    /// another lifecycle event arrives. That is a **deliberate** residual
    /// rather than an oversight: the signal that triggers a rebuild is
    /// delivered with the window already set (it is set before the callback
    /// runs), so a failure here is genuine rather than a timing race, and a
    /// poll is the wrong answer to a genuine failure — it would mean re-arming
    /// the wake hook and a backoff, a mechanism this seam does not own.
    ///
    /// Do not read the seam's statelessness as covering this. What
    /// statelessness buys is that a **missed signal** cannot strand the
    /// presentation: the next `true` re-asks, and asks about the present
    /// rather than consulting what is held. It buys nothing for a failure that
    /// has already been observed and reported, because there may be no next
    /// signal to re-ask on. The failure class that does recover on its own is
    /// a different one, and it belongs to the sibling device-loss path
    /// (`super::device_recovery`): a lost device sets the renderer's own flag,
    /// and that loop retries it under its backoff and wakes the frame loop.
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
/// Android runner's registration for the levels.
#[cfg_attr(
    not(any(test, target_os = "android")),
    expect(
        dead_code,
        reason = "driven by the Android runner and by this module's tests -- see module doc"
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
    use flui_engine::EngineError;

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

    /// The `false` edge. The second request is deliberately NOT skipped: the
    /// seam carries no state to skip with, and a `false` may be lost, so the
    /// release has to be requested from the event rather than from a
    /// remembered state. What makes the repeat harmless is the lease's own
    /// `release` (its `releasing_twice_releases_once_and_a_replacement_is_held_again`
    /// test), not anything here.
    #[test]
    fn a_release_request_reaches_the_renderer_and_a_repeat_is_not_skipped() {
        let mut backend = ScriptedSurfaceBackend::holding_a_surface();

        let first = ensure_surface(&mut backend, false);
        let second = ensure_surface(&mut backend, false);

        assert!(
            matches!(first, SurfaceLifecycleOutcome::Released),
            "a release request reports Released, got {first:?}"
        );
        assert!(
            matches!(second, SurfaceLifecycleOutcome::Released),
            "a repeated release request reports Released again, got {second:?}"
        );
        assert_eq!(
            backend.calls,
            vec![SurfaceCall::Release, SurfaceCall::Release],
            "both requests reach the renderer"
        );
        assert_eq!(backend.held, None, "the released post-state holds");
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

    /// One full cycle as the Android arms produce it — release, then rebuild
    /// — records exactly those two calls and nothing else.
    #[test]
    fn a_release_then_acquire_cycle_records_exactly_those_calls() {
        let mut backend = ScriptedSurfaceBackend::holding_a_surface();

        let released = ensure_surface(&mut backend, false);
        let rebuilt = ensure_surface(&mut backend, true);

        assert!(matches!(released, SurfaceLifecycleOutcome::Released));
        assert!(matches!(rebuilt, SurfaceLifecycleOutcome::Recreated));
        assert_eq!(
            backend.calls,
            vec![SurfaceCall::Release, SurfaceCall::Recreate],
            "the cycle is exactly a release and a recreate"
        );
        assert!(
            backend.held.is_some(),
            "the renderer presents again after the cycle"
        );
    }
}
