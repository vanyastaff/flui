#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use std::sync::Arc;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use flui_scheduler::AppLifecycleState;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::close_request::CloseRequestHandler;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::host::{
    APP_RUNTIME, runtime_needs_redraw_handle, runtime_wake_callback, with_owner_platform,
};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::realm_dispatch::{
    PlatformToUi, RealmDispatcher, RealmTask, close_this_window, dispatch_platform_realm,
    install_presentation_alongside, install_realm_alongside,
};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::AppConfig;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::runtime::WindowPolicy;

// ============================================================================
// Multi-window embedder seam (issue #555's `WindowPolicy`)
// ============================================================================

/// Opens an additional top-level window while the platform loop
/// `run_app`/`run_app_with_config` started is already running — the
/// embedder-facing seam issue #555's [`WindowPolicy`] governs which
/// realm/presentation topology the new window becomes. Must be called from
/// the owner thread while a loop is live (mirrors `bootstrap_desktop`'s own
/// `OwnerPlatform` access constraint: reachable only from inside, or after,
/// `on_ready` — e.g. from a `window.on_input`/`window.on_should_close`
/// callback the FIRST window already registered).
///
/// # [`WindowPolicy::SeparateRealms`]
///
/// Opens a fully independent second realm: its own `UiRealm`, its own
/// `GlobalKeyScope`, its own `UpdateScheduler` — installed via
/// `install_realm_alongside`, never `install_platform_realm`'s displacing
/// legacy path. `two_realms_via_separate_windows_policy_share_nothing` pins
/// the "share nothing but `SharedEngineServices`" guarantee this policy
/// claims. Input/close/should-close/focus/visibility/resize dispatch are
/// wired and addressed to this new realm exactly like the FIRST window's
/// own dispatch.
///
/// # [`WindowPolicy::SharedRealm`]
///
/// Installs a second PRESENTATION into the FIRST realm hosted on this
/// thread (via `install_presentation_alongside`) — real forest membership,
/// a real `WindowRegistry` mapping, real addressed
/// input/close/should-close/focus/visibility dispatch.
/// `one_realm_two_windows_policy_routes_by_presentation` pins that this
/// really is forest-membership routing, not a second realm in disguise.
///
/// # Completion timing: `Ready` vs `Pending`
///
/// `OwnerPlatform::open_window` resolves synchronously (`WindowOpen::Ready`)
/// only inside `on_ready`, or on a backend with no owner lane at all
/// (headless). The real winit backend defers any call after `on_ready`
/// (`WindowOpen::Pending`) — exactly the calling convention this function's
/// own doc names as its intended use (a callback the FIRST window already
/// registered). This function handles both: `Ready` installs and wires the
/// window inline; `Pending` accepts the request and completes it once the
/// owner lane resolves it, by spawning the completion on the first realm
/// hosted on this thread's own [`flui_scheduler::AsyncDriver`] (never a
/// hand-rolled busy-loop). Either way the installed topology, wiring, and
/// this doc's own guarantees are identical — only the timing differs, and a
/// `Pending` completion's own failure is traced
/// (`drain_pending_secondary_window_completions`), not silently dropped.
///
/// The completion itself never runs from inside the spawned future's own
/// poll: `UpdateScheduler::drive_async_tasks` (which polls that future) always
/// runs from INSIDE a dispatched `RealmTask::Frame`, and `WindowPolicy::
/// SharedRealm`'s own `install_presentation_alongside` has no defer-to-idle
/// queue of its own — it hard-refuses (`InstallPresentationError::
/// DispatchInFlight`) while a dispatch is in flight, which is exactly what
/// running the completion mid-poll would always trigger for that policy.
/// The future itself only enqueues the resolved window; the ACTUAL
/// install-and-wire call happens at `dispatch_platform_realm`'s own tail,
/// once this thread's checkout state clears — the same point-in-time
/// `AppRuntime`'s own deferred realm-map mutations already apply at. One
/// completion path, `finish_open_secondary_window`, for both `WindowPolicy`
/// variants; only the trigger point differs between `Ready` (inline) and
/// `Pending` (deferred to the enclosing dispatch's tail).
///
/// Tested on the headless backend's own deferred-open probe
/// (`HeadlessPlatform::enable_deferred_window_open`) for BOTH policies:
/// `open_secondary_window_completes_through_the_pending_arm_like_the_real_winit_owner_lane`
/// (`SeparateRealms`) and
/// `shared_realm_completes_through_the_pending_arm_like_the_real_winit_owner_lane`
/// (`SharedRealm` — the harder case, since its target realm IS the one
/// whose dispatch checkout the completion's own poll runs inside of) each
/// drive Pending -> resolved -> both-window close/exit end-to-end.
/// `dead_driver_realm_before_pending_resolution_disclaims_cleanly_without_zombie_installing`
/// additionally pins the fail-closed path: a driver realm torn down before
/// its own pending completion resolves drops the captured `PendingWindow`
/// along with the realm's `AsyncDriver`, disclaiming the request
/// (`flui_foundation::claim_slot`'s own `Abandoned` transition) rather than
/// zombie-installing anything later. Named residual: this proves the
/// completion seam generically, not a full winit event-loop integration
/// test (no CI gate exercises the real winit backend end-to-end at all —
/// see `flui-platform`'s own "evidence must live outside the test gate"
/// constraint) — the winit-specific gap still open is confidence that
/// `WinitOwnerHooks::open_owner_window`'s own `Pending` construction and
/// `control.rs`'s reply-delivery path compose correctly with this
/// completion, not merely that the generic completion mechanism itself
/// works.
///
/// # Named limitation (both policies): no widget content, no rendering
///
/// This window shows nothing — no root widget is mounted, no GPU renderer
/// is constructed, `window.on_request_frame` is never registered. Two
/// independent gaps, neither papered over:
///
/// - **Rendering is one-canonical-closure-per-BACKEND today, not
///   per-window.** `runner_frame_ordering.rs`'s own mechanical guards
///   (`every_runner_frame_site_uses_the_shared_drive_frame_helper`,
///   `every_pump_async_arm_calls_finish_then_drive_async_tasks`) pin
///   exactly three production `UpdateScheduler::drive_frame`/`finish_async_pump`/
///   `drive_async_tasks` call sites — desktop, Android, web — precisely to
///   catch a hand-rolled fourth copy drifting from the canonical ordering.
///   Adding a real, independently-rendering second window means
///   generalizing those three sites into one shared per-window frame-tick
///   helper (touching every existing backend's bootstrap, and this guard's
///   own pinned count) — real, scoped work belonging to its own reviewed
///   slice, not folded silently into this one under time pressure.
/// - **`UiRealm::attach_root_widget` is wired to a realm's PRIMARY
///   presentation only** (gesture arena, focus root, vsync scope all read
///   `self.presentations.primary()`); extending it to an arbitrary
///   addressed presentation is follow-up work neither the forest slice
///   (#607) nor the routing slice (#608) added — relevant to
///   `SharedRealm` specifically, `SeparateRealms`' own new presentation
///   IS its realm's primary, so that half doesn't block it, but the
///   rendering gap above still does.
///
/// What IS real and independently verifiable regardless of this gap: the
/// window opens, is live-addressed, and its registry/forest membership and
/// event dispatch (input/close/focus/visibility/resize) are genuine,
/// policy-driven, and exactly as isolated (or shared) as [`WindowPolicy`]'s
/// own doc claims.
///
/// # Errors
///
/// Window creation and (`SeparateRealms` only) `UiRealm` construction
/// surface as `Err` exactly like `bootstrap_desktop`'s own first-window
/// failures — this call does not tear down or exit the loop on failure,
/// unlike a first-window bootstrap failure (which propagates out of
/// `Platform::run` and ends the loop): the caller decides what a failed
/// secondary-window open means for their app. `SharedRealm` additionally
/// fails if no realm is hosted on this thread yet to share with.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn open_secondary_window(config: AppConfig, policy: WindowPolicy) -> anyhow::Result<()> {
    open_secondary_window_impl(config, policy).map(|_| ())
}

// Same cfg as `open_secondary_window`/`open_secondary_window_impl`/
// `finish_open_secondary_window` themselves (desktop-only) -- both statics
// exist only to serve that family, and `PENDING_SECONDARY_WINDOW_COMPLETIONS`
// names `WindowPolicy`, which is imported under this exact cfg expression
// (see this module's own `use crate::app::runtime::WindowPolicy` above). A
// narrower cfg here (e.g. `not(target_os = "ios")` alone) leaves this type
// annotation referencing an import that does not exist on android/wasm32,
// a hard compile error there, not merely dead code.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
/// One `Pending`-arm window whose open resolved, waiting for
/// [`finish_open_secondary_window`]: the policy that governs it, the native
/// window itself, and the close-request handler its caller's `AppConfig`
/// carried (threaded through so a secondary window can refuse its own close
/// exactly as the primary can).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
type PendingCompletion = (
    WindowPolicy,
    Arc<dyn flui_platform::traits::PlatformWindow>,
    Option<CloseRequestHandler>,
);

thread_local! {
    /// The pending-completion registry for `open_secondary_window`'s
    /// `Pending` arm (see [`spawn_pending_secondary_window_completion`]) —
    /// keeps each spawned [`flui_scheduler::TaskToken`] alive so its
    /// cancel-on-drop semantics don't cancel the in-flight completion the
    /// instant the spawning call returns. Append-only: an honest, documented
    /// simplification (there is no natural per-request owner to key removal
    /// on) rather than a silent unbounded-growth claim — `TaskToken::drop`
    /// still cancels cleanly on realm/thread teardown, this registry just
    /// does not proactively prune settled entries mid-run.
    static PENDING_SECONDARY_WINDOW_OPENS: std::cell::RefCell<Vec<flui_scheduler::TaskToken>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Windows resolved from `open_secondary_window`'s `Pending` arm, awaiting
    /// `finish_open_secondary_window` at the next point this thread's
    /// dispatch/hot-restart-visit checkout state is clear (see
    /// [`drain_pending_secondary_window_completions`]). Never drained
    /// in-place inside the spawned future itself: that future is polled by
    /// `UpdateScheduler::drive_async_tasks`, which always runs INSIDE a dispatched
    /// `RealmTask::Frame` (`dispatched_realm_id` is `Some` for the whole
    /// checkout) — `install_presentation_alongside`'s own
    /// `DispatchInFlight` refusal (it has no defer-to-idle queue of its own)
    /// means `WindowPolicy::SharedRealm` could never complete from inside
    /// that poll. Queuing here and draining once the checkout restores
    /// applies to BOTH policies uniformly, one completion path rather than a
    /// policy-specific carve-out.
    ///
    /// Gated exactly like [`PendingCompletion`] itself: the type does not
    /// exist on the targets that have no secondary-window path (Android,
    /// iOS, wasm32), and a `thread_local!` naming it there is a build error
    /// only the wasm-check job ever sees.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    static PENDING_SECONDARY_WINDOW_COMPLETIONS: std::cell::RefCell<Vec<PendingCompletion>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Applies every `open_secondary_window` `Pending`-arm completion queued by
/// [`spawn_pending_secondary_window_completion`]'s own future, in request
/// order. Call only from a point where this thread's dispatch/hot-restart-
/// visit checkout state is already clear (`dispatched_realm_id` and
/// `iterating_all_realms` both settled back to their idle values) — the same
/// discipline [`crate::app::runtime::AppRuntime::drain_pending_realm_mutations`]
/// requires of its own callers, and for the identical reason:
/// `finish_open_secondary_window` calls `install_presentation_alongside`/
/// `install_realm_alongside`, both of which need to actually apply rather
/// than defer (`SharedRealm`'s `install_presentation_alongside` has no
/// defer-to-idle queue of its own, so calling this before the checkout
/// clears would just reproduce the same `DispatchInFlight` refusal one level
/// up).
///
/// A completion's own failure is traced (`tracing::error!`), never
/// propagated: by the time this runs there is no synchronous caller left for
/// either policy to return an `Err` to.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn drain_pending_secondary_window_completions() {
    let pending =
        PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    for (policy, window, close_request_handler) in pending {
        if let Err(error) = finish_open_secondary_window(policy, window, close_request_handler) {
            tracing::error!(
                ?policy,
                %error,
                "open_secondary_window: a Pending-arm completion resolved to a window, but \
                 installing it failed"
            );
        }
    }
}

/// [`open_secondary_window`]'s real body, additionally returning the exact
/// native window it opened and wired — the public function discards it
/// (embedders address the window only through dispatched events, never a
/// held handle); this module's own tests need it to drive a REAL close
/// (`window.close()`) instead of reaching for the internal
/// `close_this_window`/`uninstall_platform_realm` primitives directly, which
/// would prove the primitives work without proving THIS function's own
/// `on_close` wiring calls them.
///
/// Returns `Ok(None)` for the `Pending` arm (see
/// [`spawn_pending_secondary_window_completion`]): `OwnerPlatform::
/// open_window` resolves synchronously only inside `on_ready`, or on a
/// backend with no owner lane at all (headless); the real winit backend
/// defers any call after `on_ready` — exactly the calling convention this
/// function documents as its own intended use (from a callback the FIRST
/// window already registered) — so a caller reachable ONLY through that
/// convention must handle `None` as "accepted, completing asynchronously",
/// not assume `Some` unconditionally.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn open_secondary_window_impl(
    config: AppConfig,
    policy: WindowPolicy,
) -> anyhow::Result<
    Option<(
        RealmDispatcher,
        Arc<dyn flui_platform::traits::PlatformWindow>,
    )>,
> {
    use flui_platform::{WindowOpen, WindowOptions};

    let options: WindowOptions = (&config).into();
    let open = with_owner_platform(|owner| owner.open_window(options))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "open_secondary_window called with no OwnerPlatform installed on this thread -- \
                 call only from inside, or after, a running Platform::run's on_ready"
            )
        })?
        .map_err(|error| {
            anyhow::Error::from(error).context("secondary window open request failed")
        })?;

    match open {
        WindowOpen::Ready(window) => {
            finish_open_secondary_window(policy, window, config.close_request_handler.clone())
                .map(Some)
        }
        WindowOpen::Pending(pending) => {
            spawn_pending_secondary_window_completion(
                policy,
                pending,
                config.close_request_handler.clone(),
            )?;
            Ok(None)
        }
    }
}

/// Drives `open_secondary_window`'s `Pending` arm to completion once the
/// owner lane resolves it — the fix for the P1 gap where
/// `open_secondary_window` assumed `OwnerPlatform::open_window` always
/// returns `Ready`, which the real winit owner lane never does for a call
/// after `on_ready` (`WinitOwnerHooks::open_owner_window`), the exact
/// calling convention this function's own caller documents as its intended
/// use.
///
/// Schedules the completion on a "driver realm" — the first realm hosted on
/// this thread, independent of which policy governs the NEW window (the
/// same lookup [`WindowPolicy::SharedRealm`]'s own `shared_with` uses in
/// [`open_secondary_window_impl`]) — because completing the install needs a
/// live [`flui_scheduler::UpdateScheduler`] to poll the awaited `PendingWindow` on
/// the framework's own frame-cycle-integrated async driver
/// (`AsyncDriver::spawn_local`), never a hand-rolled busy-loop: the
/// `PendingWindow`'s own wake (fired when the owner lane delivers) requests
/// a fresh frame, and that frame's `UpdateScheduler::drive_async_tasks` polls the
/// task to completion — the same mechanism any other framework-spawned
/// async task completes through.
///
/// The spawned future itself never calls [`finish_open_secondary_window`]
/// directly on success — it only enqueues `(policy, window)` onto
/// [`PENDING_SECONDARY_WINDOW_COMPLETIONS`]. `UpdateScheduler::drive_async_tasks`
/// (which polls this future to completion) always runs from INSIDE a
/// dispatched `RealmTask::Frame`, so `dispatched_realm_id` is `Some` for the
/// poll's entire duration; `WindowPolicy::SharedRealm`'s own
/// `install_presentation_alongside` has no defer-to-idle queue of its own
/// and hard-refuses (`InstallPresentationError::DispatchInFlight`) when
/// called while a dispatch is in flight. Deferring the WHOLE completion (not
/// just the presentation-install half) to
/// [`drain_pending_secondary_window_completions`] — run once the enclosing
/// dispatch's own checkout state clears — sidesteps that wall for both
/// policies uniformly, through the one completion path
/// [`finish_open_secondary_window`] already is.
///
/// # Errors
/// If no realm is hosted on this thread to drive the completion on, or if
/// dispatching to it fails — the pending window is then left unresolved;
/// the caller decides what that means for their app (this function does not
/// drop `pending` itself on that path, so the owner lane still eventually
/// observes disclaim-on-drop through the returned `anyhow::Error`'s own
/// caller, not a silent leak).
///
/// # Named residual
/// [`finish_open_secondary_window`]'s own failure, and the pending open's
/// own resolve-to-`Err` outcome, are both traced (`tracing::error!`) rather
/// than propagated — by the time the spawned future completes (or the
/// deferred drain runs) there is no synchronous caller left to return either
/// to. A richer completion signal (a callback, or a `Future` the original
/// caller can itself await) is real, scoped follow-up work, named here
/// rather than silently assumed; today's contract is "traced, never
/// silently dropped", which this function keeps.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn spawn_pending_secondary_window_completion(
    policy: WindowPolicy,
    pending: flui_platform::PendingWindow,
    close_request_handler: Option<CloseRequestHandler>,
) -> anyhow::Result<()> {
    let driver = APP_RUNTIME
        .with(|slot| {
            let state = slot.borrow();
            let (_, first_slot) = state.realms.iter().next()?;
            let owner_thread = state.owner_thread?;
            Some(RealmDispatcher {
                owner_thread,
                address: first_slot.address,
            })
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "open_secondary_window's Pending arm requires an already-hosted realm to drive \
                 its completion; none is installed on this thread"
            )
        })?;

    let future: flui_scheduler::BoxedTask = Box::pin(async move {
        match pending.await {
            Ok(window) => {
                // Enqueue, never call `finish_open_secondary_window`
                // directly here -- see this function's own doc for why
                // (this poll always runs mid-dispatch).
                PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                    queue
                        .borrow_mut()
                        .push((policy, window, close_request_handler));
                });
            }
            Err(error) => {
                tracing::error!(
                    ?policy,
                    %error,
                    "open_secondary_window: the Pending arm failed to resolve to a window"
                );
            }
        }
    });

    dispatch_platform_realm(
        driver,
        RealmTask::Frame(Box::new(move |realm| {
            let token = realm.scheduler().spawn_local(future);
            PENDING_SECONDARY_WINDOW_OPENS.with(|registry| registry.borrow_mut().push(token));
        })),
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "open_secondary_window: dispatching the Pending-arm completion task failed: {error:?}"
        )
    })
}

/// [`open_secondary_window_impl`]'s shared completion path — installs the
/// realm/presentation topology [`WindowPolicy`] governs and wires every
/// per-window callback, for a `window` that already exists (whether
/// obtained synchronously, `WindowOpen::Ready`, or asynchronously through
/// [`spawn_pending_secondary_window_completion`]'s own `Pending` resolution)
/// — so both arms install and wire a window identically, and neither
/// duplicates the other's bookkeeping.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn finish_open_secondary_window(
    policy: WindowPolicy,
    window: Arc<dyn flui_platform::traits::PlatformWindow>,
    close_request_handler: Option<CloseRequestHandler>,
) -> anyhow::Result<(
    RealmDispatcher,
    Arc<dyn flui_platform::traits::PlatformWindow>,
)> {
    use flui_platform::traits::{DispatchEventResult, PlatformInput};

    let realm_dispatch = match policy {
        WindowPolicy::SharedRealm => {
            let shared_with = APP_RUNTIME
                .with(|slot| {
                    let state = slot.borrow();
                    let (_, first_slot) = state.realms.iter().next()?;
                    let owner_thread = state.owner_thread?;
                    Some(RealmDispatcher {
                        owner_thread,
                        address: first_slot.address,
                    })
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "WindowPolicy::SharedRealm requires an already-hosted realm to share \
                         with; none is installed on this thread"
                    )
                })?;
            install_presentation_alongside(shared_with, &window).map_err(|error| {
                anyhow::Error::from(error).context("installing the secondary presentation failed")
            })?
        }
        WindowPolicy::SeparateRealms => {
            let scale_factor = window.scale_factor() as f32;
            let wake = runtime_wake_callback();
            let ui_realm = crate::app::ui_realm::UiRealm::new(
                Arc::clone(&wake),
                Arc::clone(&window),
                scale_factor,
                runtime_needs_redraw_handle(),
            )
            .map_err(|error| {
                anyhow::anyhow!(error).context("secondary UiRealm construction failed")
            })?;
            // NOT wired to a frame-failure handler: `finish_open_secondary_window`
            // never sees the primary `AppConfig` (its `config` parameter
            // upstream carries only window shape), so this secondary
            // realm's failures surface through `tracing` alone. Part of
            // issue #561's remaining work, named in ADR-0048.
            install_realm_alongside(ui_realm, &window).map_err(|error| {
                anyhow::anyhow!(error).context("installing the secondary realm failed")
            })?
        }
    };

    tracing::warn!(
        ?policy,
        ?realm_dispatch,
        "open_secondary_window: installed a live, addressed window with no widget content and no \
         renderer -- see this function's own doc for the two named, scoped-out gaps"
    );

    // Wire this presentation into the close-request seam (issue #558)
    // through the same single implementation `run_desktop`'s primary window
    // uses. Unlike the frame-failure handler noted above, this one IS
    // threaded down from the caller's `AppConfig` -- a secondary window
    // that could not refuse its own close would leave the veto reachable
    // for exactly one window per process, and a window's answer is
    // addressed to its OWN presentation, so it can never affect a
    // sibling's.
    super::install_close_request_wiring(realm_dispatch.address, &window, close_request_handler);

    window.on_input(Box::new(move |input: PlatformInput| {
        let _ =
            dispatch_platform_realm(realm_dispatch, RealmTask::Event(PlatformToUi::Input(input)));
        DispatchEventResult::resolved(false, true)
    }));

    window.on_resize(Box::new(move |size, scale_factor| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
        );
    }));

    // Window close -> close THIS window's own presentation, exactly like
    // `run_desktop`'s primary window (see `close_this_window`'s own doc):
    // `SeparateRealms` reduces to a full uninstall of this new, independent
    // realm (its sole presentation); `SharedRealm` removes just this
    // presentation from the shared realm's forest while the primary (and
    // any other sibling) survives untouched -- never a blind
    // `uninstall_platform_realm`, which would tear down the WHOLE shared
    // realm out from under a still-open sibling window.
    //
    // No `on_quit` registration here — that is a single platform-level
    // callback slot the FIRST window's bootstrap already owns
    // (`Platform::on_quit`/`SharedPlatform::on_quit` replace, never stack);
    // registering a second one here would silently steal the first window's
    // Detached-lifecycle notification on process quit instead of adding to
    // it. Generalizing quit notification to visit every hosted realm
    // (`for_each_installed_realm`) is follow-up work, named here, not
    // silently skipped.
    window.on_close(Box::new(move || {
        tracing::info!(?realm_dispatch, "Secondary window closed");
        close_this_window(realm_dispatch);
    }));
    // No `on_should_close` registration here: `install_close_request_wiring`
    // above installed it, together with the router entry it consults.
    window.on_active_status_change(Box::new(move |focused| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowFocus(focused)),
        );
    }));
    window.on_visibility_status_change(Box::new(move |visible| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowVisibility(visible)),
        );
    }));

    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
    );

    Ok((realm_dispatch, window))
}
