//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{StatelessView, View};

use super::AppConfig;

#[cfg(not(target_os = "ios"))]
use std::collections::VecDeque;
#[cfg(not(target_os = "ios"))]
use std::sync::Arc;
#[cfg(not(target_os = "ios"))]
use std::sync::atomic::AtomicBool;

#[cfg(not(target_os = "ios"))]
use flui_foundation::RealmId;
#[cfg(not(target_os = "ios"))]
use flui_scheduler::AppLifecycleState;

#[cfg(not(target_os = "ios"))]
use super::runtime::{AppRuntime, ExitPolicy, RealmSlot};
// `WindowPolicy` is named only by `open_secondary_window` (desktop-only) and
// this module's own tests.
#[cfg(all(
    not(target_os = "ios"),
    any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))
))]
use super::runtime::WindowPolicy;

/// A fresh clone of the loop-scoped platform wake capability — see
/// `AppRuntime::frame_wake_callback`'s doc. `APP_RUNTIME` must not be
/// currently mutably borrowed when this is called (it takes a shared
/// borrow); every call site here is either before a realm is installed or
/// after one has been taken out of the slot for dispatch.
#[cfg(not(target_os = "ios"))]
fn runtime_wake_callback() -> Arc<dyn Fn() + Send + Sync> {
    APP_RUNTIME.with(|slot| slot.borrow().frame_wake_callback())
}

/// A clone of the loop-scoped `needs_redraw` flag, for [`super::ui_realm::UiRealm::new`]'s
/// `needs_redraw` parameter.
#[cfg(not(target_os = "ios"))]
fn runtime_needs_redraw_handle() -> Arc<AtomicBool> {
    APP_RUNTIME.with(|slot| slot.borrow().needs_redraw_handle())
}

/// Run a FLUI application with default configuration.
///
/// This is the internal implementation called by `run_app()`.
pub fn run_app_impl<V>(root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_with_config_impl(root, AppConfig::default());
}

/// Run a FLUI application with custom configuration.
///
/// This is the internal implementation called by `run_app_with_config()`.
pub fn run_app_with_config_impl<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    // Managed startup: install FLUI's default backend only if the slot is
    // empty. An application that configured its own subscriber before calling
    // `run_app` keeps it, and a second `run_app` in one process is a no-op
    // rather than a panic.
    let _installation = super::logging::init_managed_logging(&config);

    // `target_fps` is logged as advisory, not enforced: the desktop runner's
    // steady-state pacing comes from the GPU-side blocking Fifo present
    // (`flui-engine::wgpu::Renderer::render_scene`), not from this value —
    // see `AppConfig::target_fps`'s doc for the full consumer audit.
    tracing::info!(
        title = %config.title,
        size = ?config.size,
        target_fps_advisory = config.target_fps,
        "Starting FLUI application"
    );

    // Run platform-specific event loop
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        run_desktop(root, config);
    }

    #[cfg(target_os = "android")]
    {
        let _ = (root, config);
        panic!(
            "On Android, use flui_app::run_app_android() from android_main() \
             instead of run_app(). AndroidApp must be provided by the system."
        );
    }

    #[cfg(target_os = "ios")]
    {
        run_ios(config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(root, config);
    }
}

// ============================================================================
// Loop-scoped composition root (ADR-0027, ADR-0039 §6)
// ============================================================================

#[cfg(not(target_os = "ios"))]
thread_local! {
    /// The one loop-scoped composition root, shared by desktop, Android, and
    /// wasm. Absorbs what were, before the `AppRuntime` skeleton existed, two
    /// separate thread-locals: the transitional realm host (realm slot, queue,
    /// draining, owner thread, address cache, window registry, surface
    /// applier, visible/focused) and the loop-scoped `OwnerPlatform` host —
    /// see [`AppRuntime`]'s own module doc for why one struct correctly
    /// carries both invariants. The platform callback surface still
    /// requires `Send`, so the `!Send` realm this holds remains in owner TLS
    /// until that seam is retired (ADR-0027 follow-up 5); access is only
    /// through the stamped FIFO dispatcher below and the fenced
    /// `with_owner_platform` accessor.
    ///
    /// `AppRuntime::new()` is cheap and side-effect-free (no singleton
    /// resolution) precisely so merely *touching* this thread-local -- for
    /// any reason, including `OwnerHostClearGuard::drop` firing during an
    /// unwind on a thread that never reached platform init -- can never
    /// itself trigger singleton construction or full system-font
    /// enumeration. Real service resolution happens only via the explicit
    /// `ensure_services` call in `install_platform_realm` below, when a
    /// realm is actually installed.
    pub(super) static APP_RUNTIME: std::cell::RefCell<AppRuntime> =
        std::cell::RefCell::new(AppRuntime::new());
}

/// Installs `owner` in the loop-scoped host. Call once, at the top of each
/// backend's `on_ready` callback (ADR-0039 §6) — every `run_*` entry point
/// in this module does so immediately after minting/receiving its
/// `OwnerPlatform`.
///
/// Deliberately does NOT resolve `SharedEngineServices`: `run_direct`
/// installs an owner platform and opens a window but never installs a
/// `UiRealm` (no widget tree, no painting/semantics/scheduler singleton
/// reach at all), so resolving here would pay for singleton construction
/// and full system-font enumeration on a path that can never consume
/// either. `install_platform_realm` is the one call site that resolves —
/// every realm-hosting backend goes through it, `run_direct` never does.
#[cfg(not(target_os = "ios"))]
pub(crate) fn install_owner_platform(owner: flui_platform::OwnerPlatform) {
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut().owner_platform = Some(owner);
    });
}

/// Installs the exit-policy hook this thread's `AppRuntime` consults instead
/// of letting a platform backend decide, alone, whether "every window this
/// backend tracks just closed" means "exit" — the live-loop wiring `issue
/// #555`'s `ExitPolicy`/`AppRuntime::should_exit` names as its own
/// deliberately-deferred follow-up (see `ExitPolicy`'s own doc). Call once,
/// immediately after [`install_owner_platform`], from every backend that
/// wants this: today, that is `run_desktop` only — Android/web bootstraps
/// never call this (their platforms don't override
/// [`flui_platform::traits::Platform::set_exit_policy_hook`] either, so it
/// would be inert there anyway; wiring the hook itself into those backends
/// is unrelated to whether flui-app installs it).
///
/// The hook itself re-enters `APP_RUNTIME` only when the PLATFORM calls it
/// later (a window closing) — never synchronously from this function, which
/// only registers it — so this does not violate `with_owner_platform`'s "no
/// host re-entry" rule.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "run_desktop (its one caller) is desktop-only -- android/wasm32 bootstraps \
                  never call this"
    )
)]
fn install_exit_policy_hook(policy: ExitPolicy) {
    with_owner_platform(|owner| {
        owner.shared().set_exit_policy_hook(Box::new(move || {
            let (should_exit, removed) =
                APP_RUNTIME.with(|slot| slot.borrow_mut().should_exit(policy));
            drop(removed);
            should_exit
        }));
    });
}

/// Installs the wall-clock-wake hook this thread's platform
/// backend consults, once per idle iteration, for the earliest instant it
/// should wake at instead of blocking forever — see
/// [`flui_platform::traits::Platform::set_wake_deadline_hook`]'s doc for the
/// full contract. Call once, alongside [`install_exit_policy_hook`], from
/// every backend that wants this: today, that is `run_desktop` only —
/// Android/web bootstraps never call this (their platforms don't override
/// that trait method either, so it would be inert there anyway).
///
/// The hook itself re-enters `APP_RUNTIME` only when the PLATFORM calls it
/// later (`about_to_wait`) — never synchronously from this function, which
/// only registers it — so this does not violate `with_owner_platform`'s "no
/// host re-entry" rule. Read-only (`AppRuntime::next_wake` takes `&self`),
/// unlike `install_exit_policy_hook`'s `&mut self` — no deferred-mutation
/// drain needed here, since computing a wake deadline never touches the
/// realm registry itself.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "run_desktop (its one caller) is desktop-only -- android/wasm32 bootstraps \
                  never call this"
    )
)]
fn install_wake_deadline_hook() {
    with_owner_platform(|owner| {
        owner.shared().set_wake_deadline_hook(Box::new(|| {
            APP_RUNTIME.with(|slot| slot.borrow().next_wake())
        }));
    });
}

/// Borrow-style access to the loop-scoped owner-platform capability.
/// `None` if no `OwnerPlatform` is currently installed on this thread
/// (before `on_ready`, or after the host was cleared).
///
/// Three fences (ADR-0039 §6), all landing in this one accessor:
///
/// (a) **Borrow, not clone.** `pub(crate)` to `flui-app`'s `app` module,
///     never re-exported; `OwnerPlatform` isn't `Clone`, so there is no way
///     to escape this closure with a durable owned copy — every access
///     re-crosses the fence.
/// (b) **Static scan.** This function's name carries the scanner token
///     `owner_platform`, so `scripts/check-frame-capability-scope.sh`
///     (trigger #22) mechanically rejects any call from inside
///     `build`/`perform_layout`/`paint`/composite bodies, across every
///     crate the scanner sweeps.
/// (c) **Runtime backstop.** `debug_assert!`s that the installed realm's own
///     scheduler (`AppRuntime::installed_realm_phase`) is not inside the
///     frame transaction. "Not inside a frame phase" per the ADR means
///     `TransientCallbacks`/`MidFrameMicrotasks`/`PersistentCallbacks` are
///     forbidden; `Idle` and `PostFrameCallbacks` are allowed (legitimate
///     ADR-0021-style post-frame work); `None` (truly no realm installed,
///     and none currently dispatched — see `AppRuntime::dispatched_scheduler`)
///     holds vacuously. `installed_realm_phase` reads through to the
///     checked-out realm's scheduler for the entire extent of a
///     `dispatch_platform_realm` call, not only the resident-realm case, so
///     this fence is load-bearing during a real dispatched production frame,
///     not merely when the realm sits untouched in the slot. This fence is
///     still **vacuous on binding-local frame paths**: headless/test
///     bindings drive their own binding-local `UpdateScheduler`, never a realm
///     installed into `APP_RUNTIME`, so fences (a) and (b) are the
///     load-bearing ones there — stated here, not hidden.
///
/// # `owner.shared()`'s method list is a compile-time fence too
///
/// Code reached through this accessor that isn't owner-thread-affine goes
/// through [`OwnerPlatform::shared`](flui_platform::OwnerPlatform::shared),
/// which returns `SharedPlatform` — a type whose method list IS the fence
/// (no owner-affine method, e.g. `open_window`, is ever added to it; see
/// its own rustdoc). This
/// `compile_fail` doctest is CI-run evidence for that fence: `flui-app`
/// dev/normal-depends on `flui-platform` and its own doc tests DO run under
/// `just test-doc`, unlike the equivalent illustration inside
/// `flui-platform` itself (excluded from that gate, `justfile:177`).
///
/// ```compile_fail,E0599
/// use flui_platform::headless_platform;
///
/// let _ = headless_platform().run(Box::new(|owner| {
///     let shared = owner.shared();
///     // `SharedPlatform` has no `open_window` — it stays owner-affine on
///     // `OwnerPlatform` only. Fails with "no method named `open_window`
///     // found for struct `SharedPlatform`" (E0599).
///     let _ = shared.open_window(Default::default());
///     Ok(())
/// }));
/// ```
///
/// # No host re-entry
///
/// `f` runs while this function holds an immutable `APP_RUNTIME.borrow()`.
/// Since `AppRuntime` folded the realm-facing state and `owner_platform`
/// into ONE thread-local `RefCell` (they were two disjoint cells before), `f`
/// must never call back into any function that touches this same cell:
/// `dispatch_platform_realm`,
/// `install_platform_realm`, `teardown_platform_realm`,
/// `install_surface_applier`, or `install_owner_platform` itself. Any of
/// those does `slot.borrow_mut()` while this borrow is still live, which is
/// a guaranteed `BorrowMutError` panic — in every build, not only debug
/// (`RefCell`'s borrow tracking is not a `debug_assert`). No production
/// caller does this today (`f` closures only call owner-affine `Platform`
/// methods), so this is a documented invariant with a regression pin
/// (`with_owner_platform_reentering_dispatch_panics` below), not a runtime
/// guard.
#[cfg(not(target_os = "ios"))]
pub(crate) fn with_owner_platform<R>(
    f: impl FnOnce(&flui_platform::OwnerPlatform) -> R,
) -> Option<R> {
    #[cfg(debug_assertions)]
    {
        // A sequential, separate `.with()` borrow -- released before the
        // real one below opens -- so this never re-enters the same
        // `RefCell`. `None` (no realm installed on this thread) is vacuous
        // but truthful: no realm means no frame transaction can be in
        // flight here, so the asserted property holds trivially.
        let phase = APP_RUNTIME.with(|slot| slot.borrow().installed_realm_phase());
        debug_assert!(
            !matches!(
                phase,
                Some(
                    flui_scheduler::SchedulerPhase::TransientCallbacks
                        | flui_scheduler::SchedulerPhase::MidFrameMicrotasks
                        | flui_scheduler::SchedulerPhase::PersistentCallbacks
                )
            ),
            "BUG: with_owner_platform called while the installed realm's scheduler is inside \
             the frame transaction (phase {phase:?}) -- owner_platform must \
             not be acquired from build/layout/paint (ADR-0039 §6, trigger #22)"
        );
    }
    APP_RUNTIME.with(|slot| slot.borrow().owner_platform.as_ref().map(f))
}

/// Unwind-safe TLS clearing. Arm this guard *before* calling
/// `Platform::run(...)` on any backend whose `run` returns (winit,
/// headless, Android) — not inside `on_ready` — so a panic anywhere inside
/// `on_ready` or later in `run` unwinds through the guard's `Drop` and
/// cannot leak the host into whatever runs on this thread next (notably,
/// the next test). Clearing an already-empty slot is a no-op.
///
/// Web deliberately arms no guard: the host stays resident for the page's
/// lifetime (see the web runner's own comment on this). macOS is moot:
/// `run` never returns there (`terminate:` exits the process).
///
/// # No host re-entry, and no eager resolution
///
/// `Drop` touches only `owner_platform` (`self.owner_platform.take()`) —
/// never `dispatch_platform_realm`/`install_platform_realm`/
/// `teardown_platform_realm`/`install_surface_applier`, all of which would
/// re-borrow the same `APP_RUNTIME` cell this drop already holds mutably
/// (see `with_owner_platform`'s own "No host re-entry" doc for the general
/// rule). Just as importantly, `AppRuntime::new()` is cheap and
/// side-effect-free specifically so that a bare `.borrow_mut()` here — the
/// *first* touch of `APP_RUNTIME` on a thread whose `on_ready` panicked
/// before installing anything — can never trigger `SharedEngineServices`
/// resolution (singleton construction plus full system-font enumeration)
/// while already unwinding. A panic during that resolution, on top of the
/// panic already unwinding, would abort the process instead of propagating
/// the original failure.
#[cfg(not(target_os = "ios"))]
#[must_use = "the guard must stay alive across the Platform::run(...) call it \
              guards, or the TLS host clears immediately instead of at loop exit"]
pub(crate) struct OwnerHostClearGuard {
    _private: (),
}

#[cfg(not(target_os = "ios"))]
impl OwnerHostClearGuard {
    /// Arms the guard. Call immediately before `Platform::run(...)`.
    pub(crate) fn arm() -> Self {
        Self { _private: () }
    }
}

#[cfg(not(target_os = "ios"))]
impl Drop for OwnerHostClearGuard {
    fn drop(&mut self) {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().owner_platform.take();
        });
    }
}

/// A registration-lifetime renderer-surface applier: `FnMut(size,
/// scale_factor)`. Named so [`RealmSlot`]'s `surface_applier` field
/// declaration reads plainly instead of spelling out the boxed closure type
/// inline. `pub(super)` (rather than private) so [`RealmSlot`]'s struct
/// definition in the sibling `runtime` module can name this type.
#[cfg(not(target_os = "ios"))]
pub(super) type SurfaceApplier =
    Box<dyn FnMut(flui_types::Size<flui_types::geometry::Pixels>, f32)>;

/// Restores a taken [`SurfaceApplier`] back into its realm's slot in
/// [`APP_RUNTIME`]'s registry when dropped — including during an unwinding
/// drop, so a panic inside the applier's own call (caught by
/// `dispatch_platform_realm`'s outer `catch_unwind`) cannot permanently
/// strand resizing. Without this, the applier taken out before the call is
/// simply never restored once the call panics, and every later `Resized`
/// event for that realm finds the slot empty forever, silently coalescing at
/// the `None` arm's trace instead of ever applying again.
///
/// Addressed by [`RealmId`] (not the old bare `Option`): if the realm was
/// torn down while the applier's own call was still running, restoring into
/// a now-missing slot is a silent no-op, matching this file's existing "the
/// realm may be gone by the time a destructor runs" discipline.
#[cfg(not(target_os = "ios"))]
#[must_use = "dropping this immediately restores the applier with no call in between"]
struct SurfaceApplierRestoreGuard {
    realm_id: RealmId,
    applier: Option<SurfaceApplier>,
}

#[cfg(not(target_os = "ios"))]
impl SurfaceApplierRestoreGuard {
    fn call(&mut self, size: flui_types::Size<flui_types::geometry::Pixels>, scale_factor: f32) {
        if let Some(applier) = self.applier.as_mut() {
            applier(size, scale_factor);
        }
    }
}

#[cfg(not(target_os = "ios"))]
impl Drop for SurfaceApplierRestoreGuard {
    fn drop(&mut self) {
        if let Some(applier) = self.applier.take() {
            let realm_id = self.realm_id;
            APP_RUNTIME.with(|slot| {
                if let Some(realm_slot) = slot.borrow_mut().realms.get_mut(&realm_id) {
                    realm_slot.surface_applier = Some(applier);
                }
            });
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg(not(target_os = "ios"))]
struct RealmDispatcher {
    owner_thread: std::thread::ThreadId,
    address: flui_foundation::PresentationAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(target_os = "ios"))]
enum RealmDispatchError {
    WrongThread,
    /// The realm incarnation this dispatcher was minted for is gone — the
    /// common path: `realm_id`/`presentation_id` mint from one shared
    /// counter, so teardown+reinstall always changes both, and this check
    /// (realm first) catches it before the presentation half is even
    /// compared.
    StaleRealm,
    /// The realm is live and matches, but the presentation incarnation does
    /// not — reachable today only via a forged/mixed address (a dispatcher
    /// whose presentation half was swapped for another incarnation's), and,
    /// once one realm can host more than one presentation, via real
    /// presentation replacement within a live realm. Kept as its own
    /// variant now: the design-for-N contract, not dead code.
    StalePresentation,
    RealmUnavailable,
    /// Rejected because a DIFFERENT realm is currently checked out for
    /// dispatch on this thread (issue #555): dispatch is single-threaded
    /// and sequential, so a nested dispatch that targets a realm other than
    /// the one already checked out is always a bug, never a legitimate
    /// concurrent-realm scenario — reachable only if something bypasses the
    /// defer-to-idle discipline
    /// [`super::runtime::AppRuntime::request_realm_install`]/[`super::runtime::AppRuntime::request_realm_uninstall`]
    /// exist to make unnecessary. A `debug_assert!` at the same call site
    /// makes this loud in debug builds; this variant is the release-mode
    /// fallback that still refuses instead of silently nesting.
    NestedCrossRealmDispatchRejected,
}

/// Typed, closed cross-thread payload (ADR-0037 §3): every routable
/// platform-to-UI event. Compile-time evidence that this is a real `Send`
/// boundary: `static_assertions::assert_impl_all!` is checked in this
/// module's own tests. If `PlatformInput` ever
/// stopped being `Send`, that must be fixed in `flui-platform` itself,
/// never worked around here.
// `pub(super)` because `RealmTask::Event` (also `pub(super)`, for
// `AppRuntime`'s sake) carries this type in a field the compiler considers
// reachable at that same visibility.
#[cfg(not(target_os = "ios"))]
pub(super) enum PlatformToUi {
    Input(flui_platform::traits::PlatformInput),
    Resized {
        size: flui_types::Size<flui_types::geometry::Pixels>,
        scale_factor: f32,
    },
    /// Window focus changed (winit's `WindowEvent::Focused`, or the
    /// equivalent per-backend signal; same source as the deleted `Active`
    /// variant this one replaces). Feeds the `(visible, focused)` ->
    /// `AppLifecycleState` derivation below, alongside
    /// [`WindowVisibility`](Self::WindowVisibility).
    WindowFocus(bool),
    /// Window visibility/occlusion changed (winit's `WindowEvent::Occluded`,
    /// negated — see `PlatformWindow::on_visibility_status_change`).
    ///
    /// Combined with [`WindowFocus`](Self::WindowFocus) via
    /// [`derive_lifecycle_state`] to produce the `AppLifecycleState` the
    /// ladder in [`emit_lifecycle_transition`] steps toward.
    // Not yet constructed on wasm32: `run_web` only wires `WindowFocus` —
    // no occlusion signal for the web backend yet (see run_web's comment at
    // its `on_active_status_change` registration).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    WindowVisibility(bool),
    /// Drive a lifecycle target that requires owner-local realm cleanup (most
    /// notably Detached during platform shutdown).
    Lifecycle(AppLifecycleState),
}

/// One queued unit of owner-thread work: a typed cross-thread
/// [`PlatformToUi`] event, the co-located frame pump, or a request to close
/// one presentation out of this realm's forest. `Frame` and
/// `ClosePresentation` are both deliberately KEPT OUT of the cross-thread
/// [`PlatformToUi`] vocabulary above, but for two different reasons — and
/// this enum, `RealmTask` itself, never crosses a thread either way: it is
/// owner-thread-only end to end (`AppRuntime`'s per-realm queue lives in
/// owner-thread-only `RealmSlot` storage, drained only from
/// [`dispatch_platform_realm`] on that same thread), the same as it was
/// before `ClosePresentation` existed — `Frame`'s `Box<dyn FnOnce(&UiRealm)>`
/// alone already makes the enum `!Send` in the general case, so nothing
/// about adding `ClosePresentation` changes that.
///
/// `Frame` carries an owner-local closure, which a cross-thread payload must
/// never do (ADR-0037 §3 forbids `Box<dyn FnOnce()>` on that boundary) — its
/// exclusion is load-bearing today. `ClosePresentation`'s payload alone
/// (`PresentationId`, a plain `Copy` id) happens to satisfy `Send` in
/// isolation, same as [`PlatformToUi`]'s own fields do — but that is a
/// property of the ID type, not a claim about this enum or this variant:
/// `ClosePresentation` is excluded from `PlatformToUi` because
/// [`dispatch_platform_realm`]'s own drain loop must special-case it (see
/// below), not because its payload could not cross a thread if some later
/// slice needed that.
///
/// `ClosePresentation` is handled specially by [`dispatch_platform_realm`]'s
/// own drain loop, never by [`RealmTask::run`]: closing a presentation needs
/// `&mut UiRealm` (removing it from the forest), which only exists for the
/// brief window the realm sits checked out of `APP_RUNTIME` as an owned
/// local — exactly the window the drain loop already has open, and the
/// reason this variant exists instead of giving `PresentationForest`
/// interior mutability to reach the same `&mut` from behind `run`'s shared
/// `&UiRealm` receiver.
// `pub(super)` (rather than private) so `AppRuntime`'s `queue` field, defined
// in the sibling `runtime` module, can name this type.
#[cfg(not(target_os = "ios"))]
pub(super) enum RealmTask {
    Event(PlatformToUi),
    Frame(Box<dyn FnOnce(&super::ui_realm::UiRealm)>),
    ClosePresentation(flui_foundation::PresentationId),
}

#[cfg(not(target_os = "ios"))]
impl RealmTask {
    /// Runs an `Event`/`Frame` task against the realm's shared capabilities.
    ///
    /// `presentation_id` is the [`flui_foundation::PresentationId`] the
    /// enqueueing [`RealmDispatcher`] was addressed to — stamped onto this
    /// task's queue entry at enqueue time (`dispatch_platform_realm`), since
    /// a realm's queue is shared across every presentation it hosts (issue
    /// #555 the addressed-routing slice). `Self::Frame`'s closure ignores it (the
    /// frame pump is realm-wide, not presentation-addressed); only
    /// `Self::Event` threads it through to [`PlatformToUi::run`].
    ///
    /// # Panics
    /// Panics if called with `Self::ClosePresentation` — that variant never
    /// reaches this method: [`dispatch_platform_realm`]'s drain loop matches
    /// it out before calling `run`, since it needs `&mut UiRealm` instead of
    /// this method's `&UiRealm` receiver. Not reachable from any other
    /// caller — `run` has exactly one call site.
    fn run(
        self,
        realm: &super::ui_realm::UiRealm,
        presentation_id: flui_foundation::PresentationId,
    ) {
        match self {
            Self::Event(event) => event.run(realm, presentation_id),
            Self::Frame(run) => run(realm),
            Self::ClosePresentation(id) => unreachable!(
                "BUG: RealmTask::ClosePresentation({id:?}) reached RealmTask::run -- \
                 dispatch_platform_realm's drain loop must match this variant out before \
                 calling run, so it can call close_presentation_entered with &mut UiRealm \
                 instead"
            ),
        }
    }
}

#[cfg(not(target_os = "ios"))]
impl PlatformToUi {
    /// `presentation_id` is the exact presentation this event was stamped
    /// for at enqueue time (see [`RealmTask::run`]'s doc). `Input` delivers
    /// to it through [`super::ui_realm::UiRealm::handle_input_addressed`];
    /// every other variant (`Resized`/`WindowFocus`/`WindowVisibility`/
    /// `Lifecycle`) is still realm-wide, not yet per-presentation-addressed
    /// — a stated, named gap (not silent): resize/lifecycle addressing
    /// across a genuine N>1 forest is future work, out of this hop-2 slice's
    /// bounded scope (input/IME/semantics/keyboard/redraw), except
    /// `WindowFocus(true)`, which DOES use `presentation_id` to update
    /// [`super::ui_realm::UiRealm::notify_presentation_focus_gained`] — see
    /// that arm below.
    fn run(
        self,
        realm: &super::ui_realm::UiRealm,
        presentation_id: flui_foundation::PresentationId,
    ) {
        match self {
            Self::Input(input) => realm.handle_input_addressed(presentation_id, input),
            Self::Resized { size, scale_factor } => {
                // Take the applier out of THIS realm's slot, release the
                // borrow, call it, then restore it — never call through a
                // live borrow, so a reentrant TLS access from inside the
                // applier (e.g. a nested dispatch enqueuing further work)
                // cannot hit an already-mutably-borrowed `RefCell` panic. If
                // the slot is ever found empty here (no applier installed
                // yet, or already cleared by teardown) this skips with a
                // trace instead of unwrapping/panicking; surface application
                // then coalesces onto the next real applier install.
                let realm_id = realm.realm_id();
                let applier = APP_RUNTIME.with(|slot| {
                    slot.borrow_mut()
                        .realms
                        .get_mut(&realm_id)
                        .and_then(|realm_slot| realm_slot.surface_applier.take())
                });
                match applier {
                    Some(applier) => {
                        // The guard restores the applier on drop
                        // unconditionally — including if `call` below
                        // panics and the drop runs during unwind — so a
                        // caught panic in the applier never permanently
                        // strands resizing.
                        let mut guard = SurfaceApplierRestoreGuard {
                            realm_id,
                            applier: Some(applier),
                        };
                        guard.call(size, scale_factor);
                    }
                    None => {
                        tracing::debug!(
                            "realm resize: surface applier slot is empty; surface application \
                             coalesces onto the next real applier install"
                        );
                    }
                }
                realm.set_device_pixel_ratio(scale_factor);
                realm.request_redraw();
                tracing::trace!(?size, scale_factor, "realm resize committed");
            }
            Self::WindowFocus(focused) => {
                // A `false` signal from a presentation that is NOT this
                // realm's currently ACTIVE one (`FocusCoordinator`) is a
                // stale, non-authoritative consequence of focus having
                // already moved to a DIFFERENT presentation of this SAME
                // realm (the exact scenario a realm hosting more than one
                // presentation creates) -- applying it to the `focused`
                // aggregate (still loop-scoped, not yet per-realm; see
                // `app-runtime-composition-host`'s own documented
                // limitation) would incorrectly suspend the WHOLE realm
                // while a sibling window still holds OS focus. Only the
                // ACTIVE presentation's own focus-loss is authoritative;
                // every focus GAIN always is, so this check only ever
                // applies to `focused == false`.
                if !focused && !realm.is_active_presentation(presentation_id) {
                    tracing::trace!(
                        { flui_foundation::diagnostics::PRESENTATION_ID } =
                            presentation_id.as_u64(),
                        "dropping a stale WindowFocus(false) from a presentation that is not \
                         this realm's currently active one"
                    );
                    return;
                }
                let (old, new) = APP_RUNTIME.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let old = derive_lifecycle_state(state.visible, state.focused);
                    state.focused = focused;
                    (old, derive_lifecycle_state(state.visible, state.focused))
                });
                if focused {
                    // FocusCoordinator (issue #555's addressed-routing slice): this event's own
                    // stamped presentation is the one whose window just
                    // gained OS focus -- keyboard input routes there until
                    // a DIFFERENT presentation's window reports focus next.
                    realm.notify_presentation_focus_gained(presentation_id);
                }
                emit_lifecycle_transition(realm, old, new);
            }
            Self::WindowVisibility(visible) => {
                // Presentation-level `FrameClock` gate — finer
                // grained than the loop-scoped `AppLifecycleState` derivation
                // below: `set_presentation_hidden` gates exactly the
                // presentation this event was stamped for, addressed by its
                // own `presentation_id`, independent of whether any OTHER
                // signal also flips the coarser realm-wide lifecycle state.
                // Ordered before the lifecycle derivation so an unhide's
                // wake (if any) is requested before, not after, the
                // frames-reenable redirty below might also request one —
                // `wake_frame` is idempotent to call twice in the same
                // dispatch, so this ordering is a clarity choice, not a
                // correctness requirement.
                realm.set_presentation_hidden(presentation_id, !visible);

                let (old, new) = APP_RUNTIME.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let old = derive_lifecycle_state(state.visible, state.focused);
                    state.visible = visible;
                    (old, derive_lifecycle_state(state.visible, state.focused))
                });
                emit_lifecycle_transition(realm, old, new);
            }
            Self::Lifecycle(new) => {
                emit_lifecycle_transition(realm, realm.scheduler().lifecycle_state(), new);
            }
        }
    }
}

/// Installs `applier` as `realm_id`'s registration-lifetime renderer-surface
/// applier, replacing (never stacking) any previously-installed one for that
/// SAME realm — a sibling realm's own applier is untouched. Call once per
/// realm install, alongside [`install_platform_realm`], from each backend's
/// bootstrap — never from inside a frame/event dispatch.
///
/// `realm_id` not being resident here is always a caller bug, never a
/// legitimate race: this is called synchronously, immediately alongside the
/// realm's own install, so the slot must already exist. Loud rather than a
/// silent no-op, because the failure mode otherwise is silent forever — that
/// realm's `Resized` events would coalesce onto a `None` applier for its
/// entire lifetime with nothing ever pointing at why.
#[cfg(not(target_os = "ios"))]
fn install_surface_applier(
    realm_id: RealmId,
    applier: impl FnMut(flui_types::Size<flui_types::geometry::Pixels>, f32) + 'static,
) {
    APP_RUNTIME.with(|slot| {
        if let Some(realm_slot) = slot.borrow_mut().realms.get_mut(&realm_id) {
            realm_slot.surface_applier = Some(Box::new(applier));
        } else {
            debug_assert!(
                false,
                "BUG: install_surface_applier called for realm {realm_id:?}, which is not \
                 resident in the registry -- call this once, synchronously, immediately \
                 alongside install_platform_realm/install_realm_alongside, never after"
            );
            tracing::error!(
                ?realm_id,
                "install_surface_applier: realm not found in the registry -- this realm's \
                 resize handling is silently lost for its whole lifetime"
            );
        }
    });
}

// ============================================================================
// Lifecycle derivation and ladder synthesis (see ADR-0035)
// ============================================================================

/// Derives the Flutter-parity [`AppLifecycleState`] from the two window
/// signals FLUI tracks per window: visibility (occlusion) and focus.
///
/// Pure and order-insensitive: the result depends only on the final
/// `(visible, focused)` pair, never on which of the two changed most
/// recently — occlusion-before-focus-loss and focus-loss-before-occlusion
/// converge to the same derived state once both signals have landed.
#[cfg(not(target_os = "ios"))]
fn derive_lifecycle_state(visible: bool, focused: bool) -> AppLifecycleState {
    if !visible {
        AppLifecycleState::Hidden
    } else if focused {
        AppLifecycleState::Resumed
    } else {
        AppLifecycleState::Inactive
    }
}

/// The intermediate `AppLifecycleState` steps between `old` and `new`,
/// inclusive of `new`, exclusive of `old`.
///
/// Faithful port of `ServicesBinding._generateStateTransitions`
/// (`packages/flutter/lib/src/services/binding.dart` @ 3.44.0) — NOT a walk
/// over this enum's own `#[repr(u8)]` discriminants, which exist for
/// FLUI's `frames_enabled` derivation and do not match Flutter's ladder
/// order. Flutter's `dart:ui` `AppLifecycleState` enum declares `detached`
/// **first** (`engine/.../platform_dispatcher.dart`: `detached, resumed,
/// inactive, hidden, paused` — `detached` is the state the engine starts in
/// *before* initialization, not a terminal "highest" state), which is
/// exactly [`AppLifecycleState::ALL`]'s order — the array this function
/// walks, not `as u8`.
///
/// Three cases, mirroring the oracle exactly:
/// - **Target is `Detached`**: walk forward from `old` to the end of `ALL`
///   (through every remaining non-detached state), then append `Detached`
///   itself. This is Flutter's dedicated `state == detached` branch — going
///   to `Detached` always visits every state after `old`, regardless of
///   where `old` sits.
/// - **Going backward** (`old`'s index > `new`'s index, e.g. `Paused` ->
///   `Resumed`): the intermediate states in *descending* index order,
///   ending at `new` (Flutter's `insert(0, ...)` loop, which prepends and
///   so reverses the ascending walk).
/// - **Going forward** (otherwise): the intermediate states in ascending
///   index order, ending at `new`.
///
/// Because `Detached` sits at index 0 (the lowest), a transition FROM
/// `Detached` to anything else always takes the forward branch: `Detached
/// -> Resumed` is the single step `[Resumed]`, not a crawl through
/// `Paused`/`Hidden`/`Inactive` first — reachable via Android's Pause/Resume
/// reroute if `UpdateScheduler::lifecycle_state()`'s corrupt-byte fallback
/// (`try_from_u8`'s `unwrap_or(AppLifecycleState::Detached)`) is ever hit.
///
/// Returns an empty `Vec` when `old == new` — this is where change-detection
/// for the whole re-derivation lives: a wake that doesn't change the derived
/// state emits nothing, to neither the scheduler nor `WidgetsBinding`
/// observers.
#[cfg(not(target_os = "ios"))]
fn lifecycle_ladder(old: AppLifecycleState, new: AppLifecycleState) -> Vec<AppLifecycleState> {
    if old == new {
        return Vec::new();
    }

    let order = AppLifecycleState::ALL;
    let old_idx = order
        .iter()
        .position(|&s| s == old)
        .expect("BUG: every AppLifecycleState variant must appear in AppLifecycleState::ALL");
    let new_idx = order
        .iter()
        .position(|&s| s == new)
        .expect("BUG: every AppLifecycleState variant must appear in AppLifecycleState::ALL");

    if new == AppLifecycleState::Detached {
        let mut steps: Vec<AppLifecycleState> = order[old_idx + 1..].to_vec();
        steps.push(AppLifecycleState::Detached);
        steps
    } else if old_idx > new_idx {
        order[new_idx..old_idx].iter().rev().copied().collect()
    } else {
        order[old_idx + 1..=new_idx].to_vec()
    }
}

/// Emits the full ladder from `old` to `new` (see [`lifecycle_ladder`]), one
/// step at a time, to both the realm's own `UpdateScheduler` and its
/// `WidgetsBinding` observers — mirroring Flutter's single platform-message
/// stream driving both `SchedulerBinding` and `WidgetsBinding` from the same
/// synthesized sequence of states.
///
/// Installed as a direct call in the same `PlatformToUi` handler (never an
/// `UpdateScheduler`-listener closure): a listener captured at bootstrap time
/// would have to resolve `realm`/`WidgetsBinding` lazily at fire time,
/// which is unsound here specifically because every production caller of
/// this function runs from inside `dispatch_platform_realm`'s dispatch
/// window — the window during which the realm is taken OUT of
/// `APP_RUNTIME` and only restored once the dispatched task returns. A
/// listener resolving `APP_RUNTIME` at fire time would see `None` on every
/// real transition and silently no-op (this shipped once and was caught by
/// `frames_reenable_redirties_root_when_dispatched_through_the_realm_queue`
/// in `realm_dispatch_tests`, which reproduces via a real dispatched
/// `PlatformToUi::Lifecycle` sequence rather than driving `UpdateScheduler`
/// directly). `realm` is already in scope here (`PlatformToUi::run`'s
/// parameter), so no such resolution is ever needed — the frames-reenable
/// redirty below reads and writes it directly, in the same stack frame
/// that owns it for the whole call.
#[cfg(not(target_os = "ios"))]
fn emit_lifecycle_transition(
    realm: &super::ui_realm::UiRealm,
    old: AppLifecycleState,
    new: AppLifecycleState,
) {
    let mut first_panic = None;
    for step in lifecycle_ladder(old, new) {
        let presentation_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            realm.handle_presentation_lifecycle(step);
        }))
        .err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            presentation_panic,
            "presentation lifecycle transition",
        );

        let gesture_cleanup_panic = if matches!(
            step,
            AppLifecycleState::Hidden | AppLifecycleState::Paused | AppLifecycleState::Detached
        ) {
            // A hidden or suspended platform is not required to send the Up
            // or Cancel matching an in-flight Down. Drain this realm's input
            // transaction before lifecycle observers can retain stale gesture
            // state into the next visible frame.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                realm.gestures().handle_lifecycle_pause();
            }))
            .err()
        } else {
            None
        };
        preserve_first_lifecycle_panic(&mut first_panic, gesture_cleanup_panic, "gesture cleanup");

        // Frames-disabled->enabled re-dirty: FLUI has no retained-scene
        // re-present, so an app that was `Hidden`/`Paused`/`Detached` and
        // comes back to `Resumed`/`Inactive` needs the root explicitly
        // re-dirtied, or the next frame finds nothing dirty and silently
        // stays Idle instead of repainting the stale window. Read
        // `frames_enabled()` immediately before and after the scheduler
        // call below so this observes exactly the edge THIS step produced,
        // whichever named state it is — `handle_app_lifecycle_state_change`
        // flips the flag via one atomic swap per call, so bracketing a
        // single call this way cannot miss or double-count an edge.
        let frames_were_enabled = realm.scheduler().frames_enabled();

        let scheduler_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            realm.scheduler().handle_app_lifecycle_state_change(step);
        }))
        .err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            scheduler_panic,
            "scheduler lifecycle dispatch",
        );

        if !frames_were_enabled && realm.scheduler().frames_enabled() {
            let redirty_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                realm.redirty_root_for_frames_reenable();
                realm.wake_frame();
            }))
            .err();
            preserve_first_lifecycle_panic(
                &mut first_panic,
                redirty_panic,
                "frames-reenable redirty",
            );
        }

        let widgets_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            realm.widgets().handle_app_lifecycle_state_changed(step);
        }))
        .err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            widgets_panic,
            "widgets lifecycle dispatch",
        );
    }

    // Every sink has now observed (or attempted) the complete synthesized
    // ladder. The earliest payload keeps transaction ordering deterministic.
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

#[cfg(not(target_os = "ios"))]
fn preserve_first_lifecycle_panic(
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    candidate: Option<Box<dyn std::any::Any + Send>>,
    phase: &'static str,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if first.is_none() {
        *first = Some(candidate);
    } else {
        tracing::error!(
            phase,
            "lifecycle phase panicked after an earlier phase; only the first panic is resumed"
        );
        // A secondary user panic may carry a payload whose destructor also
        // panics. Leaking that exceptional payload prevents it from replacing
        // the first lifecycle failure or aborting while the first unwinds.
        std::mem::forget(candidate);
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod lifecycle_derivation_tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use flui_interaction::{
        HitTestEntry, HitTestResult, InteractionLane, RenderId,
        events::{PointerType, make_down_event, make_move_event},
    };
    use flui_types::geometry::{Offset, Pixels};
    use flui_view::WidgetsBindingObserver;

    use super::{
        AppLifecycleState, derive_lifecycle_state, emit_lifecycle_transition, lifecycle_ladder,
    };

    struct GestureStateObserver {
        cleanup_committed: Arc<AtomicBool>,
        hidden_saw_cleanup: AtomicBool,
    }

    impl WidgetsBindingObserver for GestureStateObserver {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            if state == AppLifecycleState::Hidden {
                self.hidden_saw_cleanup.store(
                    self.cleanup_committed.load(Ordering::Acquire),
                    Ordering::Release,
                );
            }
        }
    }

    struct LifecycleSeen(Mutex<Vec<AppLifecycleState>>);

    impl WidgetsBindingObserver for LifecycleSeen {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            self.0.lock().expect("lifecycle log lock").push(state);
        }
    }

    struct SetCleanupOnDrop(Arc<AtomicBool>);

    impl Drop for SetCleanupOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    struct PanicOnLifecycleRouteDrop;

    impl Drop for PanicOnLifecycleRouteDrop {
        fn drop(&mut self) {
            panic!("lifecycle route cleanup panic");
        }
    }

    struct PanickingLifecycleObserver(Arc<AtomicBool>);

    impl WidgetsBindingObserver for PanickingLifecycleObserver {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            if state == AppLifecycleState::Hidden {
                self.0.store(true, Ordering::Release);
                panic!("widgets lifecycle listener panic");
            }
        }
    }

    #[test]
    fn derivation_truth_table() {
        assert_eq!(
            derive_lifecycle_state(true, true),
            AppLifecycleState::Resumed
        );
        assert_eq!(
            derive_lifecycle_state(true, false),
            AppLifecycleState::Inactive
        );
        assert_eq!(
            derive_lifecycle_state(false, true),
            AppLifecycleState::Hidden,
            "not visible must win over focused — a hidden window cannot be Resumed"
        );
        assert_eq!(
            derive_lifecycle_state(false, false),
            AppLifecycleState::Hidden
        );
    }

    /// Occlusion-before-focus-loss and focus-loss-before-occlusion must
    /// converge to the same derived state — the derivation depends only on
    /// the final `(visible, focused)` pair, never on update order.
    /// Mirrors `AppRuntime`'s actual update pattern (mutate one signal,
    /// re-derive) so this test exercises real ordering, not just two calls
    /// to a pure function with identical arguments.
    struct WindowSignals {
        visible: bool,
        focused: bool,
    }

    impl WindowSignals {
        fn new() -> Self {
            Self {
                visible: true,
                focused: true,
            }
        }

        fn set_visible(&mut self, visible: bool) -> AppLifecycleState {
            self.visible = visible;
            derive_lifecycle_state(self.visible, self.focused)
        }

        fn set_focused(&mut self, focused: bool) -> AppLifecycleState {
            self.focused = focused;
            derive_lifecycle_state(self.visible, self.focused)
        }
    }

    #[test]
    fn derivation_is_order_insensitive() {
        // Occlusion before focus loss.
        let mut occlusion_first = WindowSignals::new();
        let _after_occlusion = occlusion_first.set_visible(false);
        let occlusion_then_focus_loss = occlusion_first.set_focused(false);

        // The same two updates, reverse order: focus loss before occlusion.
        let mut focus_loss_first = WindowSignals::new();
        let _after_focus_loss = focus_loss_first.set_focused(false);
        let focus_loss_then_occlusion = focus_loss_first.set_visible(false);

        assert_eq!(
            occlusion_then_focus_loss, focus_loss_then_occlusion,
            "both orderings of the same two updates must land on the same derived state"
        );
        assert_eq!(occlusion_then_focus_loss, AppLifecycleState::Hidden);
    }

    #[test]
    fn ladder_is_empty_for_an_unchanged_state() {
        assert!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Resumed).is_empty(),
            "a no-op transition must emit nothing — this is where change-detection for the \
             whole re-derivation lives (neither the scheduler nor WidgetsBinding observers see \
             a same-state call)"
        );
        assert!(lifecycle_ladder(AppLifecycleState::Hidden, AppLifecycleState::Hidden).is_empty());
    }

    /// Pause's ladder: Resumed -> Paused must visit Inactive, then Hidden,
    /// then Paused, in that order.
    #[test]
    fn ladder_steps_forward_through_every_intermediate_state_in_order() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Paused),
            vec![
                AppLifecycleState::Inactive,
                AppLifecycleState::Hidden,
                AppLifecycleState::Paused,
            ]
        );
    }

    /// Resume's ladder: the exact reverse of Pause's.
    #[test]
    fn ladder_steps_backward_through_every_intermediate_state_in_order() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Paused, AppLifecycleState::Resumed),
            vec![
                AppLifecycleState::Hidden,
                AppLifecycleState::Inactive,
                AppLifecycleState::Resumed,
            ]
        );
    }

    #[test]
    fn ladder_single_step_transitions_emit_exactly_that_step() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Inactive),
            vec![AppLifecycleState::Inactive]
        );
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Inactive, AppLifecycleState::Resumed),
            vec![AppLifecycleState::Resumed]
        );
    }

    /// Regression: `Detached` sits FIRST in Flutter's real `AppLifecycleState`
    /// order (`AppLifecycleState::ALL`: `Detached, Resumed, Inactive, Hidden,
    /// Paused` — the engine's "before initialization" state), not last. A
    /// transition FROM `Detached` is therefore a single forward step to
    /// whatever `new` is, never a crawl through every OTHER state first — the
    /// oracle's dedicated `state == detached` branch only fires when
    /// `Detached` is the TARGET, not the source.
    ///
    /// Reachable via Android's Pause/Resume reroute if `UpdateScheduler::
    /// lifecycle_state()`'s corrupt-byte fallback (`try_from_u8`'s
    /// `unwrap_or(AppLifecycleState::Detached)`) is ever hit as "old".
    #[test]
    fn ladder_from_detached_is_a_single_forward_step() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Detached, AppLifecycleState::Resumed),
            vec![AppLifecycleState::Resumed],
            "Detached -> Resumed must NOT synthesize Paused/Hidden/Inactive first"
        );
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Detached, AppLifecycleState::Inactive),
            vec![AppLifecycleState::Resumed, AppLifecycleState::Inactive]
        );
    }

    /// `Detached` as the TARGET is the oracle's special case: walk every
    /// remaining state after `old`, in order, then append `Detached` itself.
    #[test]
    fn ladder_to_detached_walks_every_remaining_state_then_appends_detached() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Detached),
            vec![
                AppLifecycleState::Inactive,
                AppLifecycleState::Hidden,
                AppLifecycleState::Paused,
                AppLifecycleState::Detached,
            ]
        );
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Hidden, AppLifecycleState::Detached),
            vec![AppLifecycleState::Paused, AppLifecycleState::Detached]
        );
    }

    #[test]
    fn hidden_transition_drains_the_realms_interrupted_pointer_sequence() {
        let realm = super::super::ui_realm::UiRealm::for_test();
        let lane = InteractionLane::try_new().expect("test interaction lane");
        let handle = lane.dispatch_handle();
        let cleanup_committed = Arc::new(AtomicBool::new(false));
        let observer = Arc::new(GestureStateObserver {
            cleanup_committed: Arc::clone(&cleanup_committed),
            hidden_saw_cleanup: AtomicBool::new(false),
        });
        let observer_handle: Arc<dyn WidgetsBindingObserver> = observer.clone();
        realm.widgets().add_observer(observer_handle.clone());

        realm.enter(|realm| {
            lane.enter(|| {
                realm
                    .gestures()
                    .set_resampling_enabled(true)
                    .expect("test realm has no active pointer before configuration");
                let owner = SetCleanupOnDrop(Arc::clone(&cleanup_committed));
                let target = handle
                    .register_pointer(move |_| {
                        let _keep_owner_alive = &owner;
                    })
                    .expect("register lifecycle target");
                let mut result = HitTestResult::new();
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                let down =
                    make_down_event(Offset::new(Pixels(8.0), Pixels(13.0)), PointerType::Touch);
                realm.gestures().handle_pointer_event(&down, |_| result);
                let move_event =
                    make_move_event(Offset::new(Pixels(9.0), Pixels(14.0)), PointerType::Touch);
                realm
                    .gestures()
                    .handle_pointer_event(&move_event, |_| HitTestResult::new());
                handle
                    .unregister_pointer(target)
                    .expect("cached route retains lifecycle target");
                assert_eq!(realm.gestures().active_pointer_count(), 1);
                assert_eq!(realm.gestures().active_resampler_count(), 1);
                assert_eq!(realm.gestures().pending_move_count(), 1);

                emit_lifecycle_transition(
                    realm,
                    AppLifecycleState::Resumed,
                    AppLifecycleState::Hidden,
                );

                assert_eq!(realm.gestures().active_pointer_count(), 0);
                assert_eq!(realm.gestures().active_resampler_count(), 0);
                assert_eq!(realm.gestures().pending_move_count(), 0);
                assert!(realm.gestures().arena().is_empty());
                assert!(
                    observer.hidden_saw_cleanup.load(Ordering::Acquire),
                    "lifecycle observers must see gesture teardown already committed"
                );

                // Restore Resumed before this test-local realm drops -- tidy,
                // not required for isolation (each realm owns its own
                // scheduler now, so there is nothing left to leak between
                // tests).
                emit_lifecycle_transition(
                    realm,
                    AppLifecycleState::Hidden,
                    AppLifecycleState::Resumed,
                );
            });
        });
        realm.widgets().remove_observer(&observer_handle);
    }

    #[test]
    fn multi_step_lifecycle_commits_the_target_before_the_first_panic_resumes() {
        let realm = super::super::ui_realm::UiRealm::for_test();
        let lane = InteractionLane::try_new().expect("test interaction lane");
        let handle = lane.dispatch_handle();
        let observer = Arc::new(LifecycleSeen(Mutex::new(Vec::new())));
        let observer_handle: Arc<dyn WidgetsBindingObserver> = observer.clone();
        realm.widgets().add_observer(observer_handle.clone());
        let scheduler_listener_panicked = Arc::new(AtomicBool::new(false));
        let scheduler_probe = Arc::clone(&scheduler_listener_panicked);
        let scheduler_listener =
            realm
                .scheduler()
                .add_lifecycle_state_listener(Arc::new(move |state| {
                    if state == AppLifecycleState::Hidden {
                        scheduler_probe.store(true, Ordering::Release);
                        panic!("scheduler lifecycle listener panic");
                    }
                }));
        let widget_listener_panicked = Arc::new(AtomicBool::new(false));
        let panicking_observer: Arc<dyn WidgetsBindingObserver> = Arc::new(
            PanickingLifecycleObserver(Arc::clone(&widget_listener_panicked)),
        );
        realm.widgets().add_observer(panicking_observer.clone());

        realm.enter(|realm| {
            lane.enter(|| {
                let owner = PanicOnLifecycleRouteDrop;
                let target = handle
                    .register_pointer(move |_| {
                        let _keep_owner_alive = &owner;
                    })
                    .expect("register lifecycle target");
                let mut result = HitTestResult::new();
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                let down =
                    make_down_event(Offset::new(Pixels(3.0), Pixels(5.0)), PointerType::Touch);
                realm.gestures().handle_pointer_event(&down, |_| result);
                handle
                    .unregister_pointer(target)
                    .expect("cached route retains lifecycle target");

                let unwind = catch_unwind(AssertUnwindSafe(|| {
                    emit_lifecycle_transition(
                        realm,
                        AppLifecycleState::Resumed,
                        AppLifecycleState::Paused,
                    );
                }));
                let payload = unwind.expect_err("route cleanup panic must propagate");

                assert_eq!(
                    payload.downcast_ref::<&str>(),
                    Some(&"lifecycle route cleanup panic")
                );
                assert_eq!(
                    *observer.0.lock().expect("lifecycle log lock"),
                    vec![
                        AppLifecycleState::Inactive,
                        AppLifecycleState::Hidden,
                        AppLifecycleState::Paused,
                    ],
                    "the complete synthesized ladder must reach widget observers"
                );
                assert_eq!(realm.gestures().active_pointer_count(), 0);
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Paused,
                    "the target state must commit before the first panic resumes"
                );
                assert!(
                    scheduler_listener_panicked.load(Ordering::Acquire),
                    "scheduler lifecycle sink must run after cleanup"
                );
                assert!(
                    widget_listener_panicked.load(Ordering::Acquire),
                    "widgets lifecycle sink must run after a scheduler listener panic"
                );

                assert!(
                    realm
                        .scheduler()
                        .remove_lifecycle_state_listener(scheduler_listener),
                    "test scheduler listener must be removable"
                );
                realm.widgets().remove_observer(&panicking_observer);
                realm.widgets().remove_observer(&observer_handle);
                emit_lifecycle_transition(
                    realm,
                    AppLifecycleState::Paused,
                    AppLifecycleState::Resumed,
                );
            });
        });
    }
}

/// Installs `realm` as the SOLE hosted realm on this thread, minting its
/// dispatcher's address by registering `window` in the single
/// [`super::window_registry::WindowRegistry`] authority — the registry is
/// the sole mint path for a routable [`flui_foundation::PresentationAddress`];
/// no caller of this function ever names the platform-internal native-handle
/// key type itself.
///
/// This is the legacy single-primary-realm entry point every backend's
/// bootstrap still calls exactly once: it clears the ENTIRE realm registry
/// (every hosted realm, not only one) before inserting the fresh one —
/// exactly the behavior the single `Option<UiRealm>` slot this registry
/// replaces used to have, since that slot could only ever hold one realm at
/// all. A second, non-displacing realm (a genuinely independent window
/// alongside this one) is installed through
/// [`install_realm_alongside`] instead.
#[cfg(not(target_os = "ios"))]
fn install_platform_realm(
    realm: super::ui_realm::UiRealm,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> RealmDispatcher {
    let owner_thread = std::thread::current().id();
    let address = flui_foundation::PresentationAddress {
        realm_id: realm.realm_id(),
        presentation_id: realm.presentation_id(),
    };
    let displaced = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Every realm hosted here may already be installed — a reinstall
        // without an intervening `teardown_platform_realm` (the
        // panic-recovery path: a mid-`on_ready` failure leaves the old
        // registry/queue/applier/window-registry mappings in place, and
        // bootstrap tries again on the same thread). Remove every registry
        // mapping addressed to EACH displaced realm — not just the window
        // being installed now — in this same borrow, before registering the
        // new window: otherwise a displaced realm's own window(s) survive as
        // dead entries no later teardown ever reaches (this legacy entry
        // point is the only one that clears the whole registry at once).
        let mut removed_window_mappings = 0;
        let displaced = state.realms.clear();
        for (displaced_id, _) in &displaced {
            removed_window_mappings += state.registry.remove_realm(*displaced_id).len();
        }
        state.registry.register_window(window, address);

        if !displaced.is_empty() {
            tracing::warn!(
                displaced_realms = displaced.len(),
                new_address = ?address,
                removed_window_mappings,
                "install_platform_realm: replacing realm(s) that were never torn down"
            );
        }
        state.realms.insert(
            address.realm_id,
            RealmSlot {
                realm: Some(realm),
                queue: VecDeque::new(),
                draining: false,
                address,
                surface_applier: None,
            },
        );
        state.owner_thread = Some(owner_thread);
        // Defensive: a reinstall-without-teardown only reaches this path
        // when the displaced incarnation's own dispatch never restored its
        // slot's `realm` (the panic-recovery scenario this function's doc
        // already documents) — `dispatched_scheduler`/`dispatched_realm_id`,
        // if the displaced incarnation left either stashed, belong to that
        // dead incarnation and must not leak into the fresh one's fence-(c)
        // reads.
        state.dispatched_scheduler = None;
        state.dispatched_realm_id = None;
        // A second realm installed on this thread (hot-restart, or a
        // sequential test realm) must not inherit whatever `(visible,
        // focused)` the PREVIOUS realm's window last reported — every
        // backend starts a window visible and focused (see `AppRuntime::new`
        // and each `MockWindow`/`WinitWindow` constructor), so a fresh
        // realm's derivation must start from that same baseline, not a
        // stale `Hidden`/`Inactive` left behind by the last one.
        state.visible = true;
        state.focused = true;
        // Explicit, known-point resolution: a realm is actually being
        // installed, so this thread genuinely needs `SharedEngineServices`
        // -- unlike `install_owner_platform`, which every backend calls
        // (including `run_direct`, which never installs a realm and never
        // needs these services). Idempotent (`ensure_services` caches), so
        // it does not matter whether a prior realm on this thread already
        // triggered it.
        let _ = state.ensure_services();
        displaced
    });
    // Destructors may re-enter platform/framework code (the same invariant
    // `teardown_platform_realm` honors) — drop only after the TLS borrow
    // above has released.
    drop(displaced);
    RealmDispatcher {
        owner_thread,
        address,
    }
}

/// Installs `realm` ALONGSIDE whatever is already hosted, never displacing a
/// sibling — the multi-realm counterpart to [`install_platform_realm`]'s
/// legacy single-primary-realm replace semantics. Requests window
/// registration and the registry insertion TOGETHER, through
/// [`super::runtime::AppRuntime::request_realm_install`] (never registers
/// the window separately/eagerly — see that method's own doc for the gap a
/// two-step sequence would leave open), so an install requested while
/// another realm is checked out for dispatch (a frame callback opening a
/// second window) defers to loop idle instead of installing mid-dispatch.
///
/// `Err(RegistryError::WindowAlreadyMapped)` only when applied immediately
/// (no dispatch/visit in flight) and `window`'s id already maps to a live
/// entry — refused, never silently re-routed onto whichever sibling realm
/// already owns that id (`WindowRegistry::register_window`'s replace
/// semantics is exactly the wrong tool for two realms meant to coexist). A
/// deferred install that later collides is traced and dropped instead (see
/// `AppRuntime::drain_pending_realm_mutations`), since the caller has
/// already returned by the time a deferred mutation applies.
///
/// Production caller: [`open_secondary_window`] under
/// [`super::runtime::WindowPolicy::SeparateRealms`] — the embedder-facing
/// seam issue #555 adds. Also exercised directly by this module's own
/// tests.
///
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "open_secondary_window (its production caller) is desktop-only -- android/wasm32 \
                  have no caller outside this module's own tests"
    )
)]
fn install_realm_alongside(
    realm: super::ui_realm::UiRealm,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> Result<RealmDispatcher, super::window_registry::RegistryError> {
    let owner_thread = std::thread::current().id();
    let address = flui_foundation::PresentationAddress {
        realm_id: realm.realm_id(),
        presentation_id: realm.presentation_id(),
    };
    let window = std::sync::Arc::clone(window);
    let result = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.owner_thread.get_or_insert(owner_thread);
        let _ = state.ensure_services();
        state.request_realm_install(
            address.realm_id,
            RealmSlot {
                realm: Some(realm),
                queue: VecDeque::new(),
                draining: false,
                address,
                surface_applier: None,
            },
            window,
        )
    });
    // On a refused collision, `result` carries the rejected `RealmSlot` (and
    // the `UiRealm` it owns) back out of the TLS borrow above -- dropped
    // only here, after that borrow has released, never inside it (see
    // `AppRuntime::apply_install`'s own doc for why).
    match result {
        Ok(()) => Ok(RealmDispatcher {
            owner_thread,
            address,
        }),
        Err((error, rejected_slot)) => {
            drop(rejected_slot);
            Err(error)
        }
    }
}

/// Errors from [`install_presentation_alongside`]. A dedicated type rather
/// than folding into [`RealmDispatchError`]: none of that enum's variants
/// mean "the realm is fine, but this specific window id collided" —
/// mislabeling that as `RealmUnavailable` (the pre-fix shape) told a caller
/// the wrong thing about what actually went wrong.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "install_presentation_alongside (its only producer) is desktop-only -- \
                  android/wasm32 have no caller outside this module's own tests"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum InstallPresentationError {
    /// `dispatcher`'s realm no longer exists (a newer realm replaced it, or
    /// it was already torn down).
    #[error("the realm this dispatcher was minted for no longer exists")]
    RealmUnavailable,
    /// A dispatch or hot-restart visit is currently in flight on this
    /// thread; see this function's own doc for why that is a named,
    /// stated gap rather than a defer-to-idle path.
    #[error("a dispatch or hot-restart visit is in flight on this thread")]
    DispatchInFlight,
    /// `dispatcher`'s realm is live, but `dispatcher.address` itself is no
    /// longer registered in `WindowRegistry` — the presentation it was
    /// minted for closed since, even though a DIFFERENT presentation kept
    /// the realm alive. The same authorization check
    /// `dispatch_platform_realm` runs (`registry.contains_address`), applied
    /// here too: a caller must hold a dispatcher whose exact address is
    /// CURRENTLY live to authorize installing another presentation
    /// alongside it, not merely one whose realm happens to still exist.
    #[error("the presentation this dispatcher was minted for is no longer registered")]
    StalePresentation,
    /// `window`'s id was already registered to a (possibly different)
    /// address — practically unreachable for a freshly opened window, but
    /// a real, distinct failure mode from `RealmUnavailable`: the realm
    /// itself is perfectly fine.
    #[error("window is already registered: {0}")]
    WindowAlreadyMapped(#[from] super::window_registry::RegistryError),
}

/// Installs another presentation into `dispatcher`'s realm, alongside
/// whatever it already hosts — the addressed-routing counterpart to
/// [`install_realm_alongside`] (which installs a second REALM instead of a
/// second presentation of the SAME realm). This is the production entry
/// point [`super::presentation_forest::PresentationForest`]'s doc and
/// `docs/runtime-contract.toml`'s `multi-presentation-forest-gate` entry
/// both point to: the forest's former `len()<=1` ratchet lifted (issue #555)
/// specifically so this function has somewhere real to install into.
///
/// `window` becomes the fresh presentation's own native window. Ordering is
/// load-bearing: the presentation is
/// [`assembled`](super::ui_realm::UiRealm::assemble_presentation) but NOT
/// yet installed into the forest, then its `WindowRegistry` mapping is
/// minted, and ONLY on success is it
/// [`installed`](super::ui_realm::UiRealm::install_presentation) — so a
/// registration failure leaves nothing forest-resident to roll back (the
/// assembled-but-uninstalled `PresentationState` is simply dropped), never
/// a presentation the forest holds with no registry entry of its own. This
/// is the single-native-window-map-authority invariant this function
/// exists to uphold: no hosted presentation is ever forest-resident
/// without a mapping.
///
/// # Errors
///
/// [`InstallPresentationError::RealmUnavailable`] if `dispatcher`'s realm no
/// longer exists. [`InstallPresentationError::StalePresentation`] if the
/// realm survives but `dispatcher.address` itself is no longer a live
/// registered address (its own presentation closed, even though a sibling
/// kept the realm alive) — the same authorization
/// `dispatch_platform_realm` requires of every dispatched task, applied to
/// this mutation too. [`InstallPresentationError::WindowAlreadyMapped`] if
/// `window`'s id is somehow already registered (practically unreachable: a
/// freshly opened window has a fresh id by construction) — nothing is
/// installed into the forest in this case.
/// [`InstallPresentationError::DispatchInFlight`] if a dispatch or
/// hot-restart visit is currently in flight on this thread: **named gap,
/// not a silent one** — unlike [`install_realm_alongside`]/
/// [`uninstall_platform_realm`], this path does not yet defer to loop idle
/// through `AppRuntime::pending_realm_mutations`; [`open_secondary_window`]
/// (its production caller, under
/// [`super::runtime::WindowPolicy::SharedRealm`]) never calls this from
/// inside a dispatched callback, so there is nothing forcing that
/// generalization today. Extending the deferral queue to
/// presentation-install requests is follow-up work, not silently skipped:
/// this refuses loudly (a `debug_assert!` in debug builds) rather than
/// corrupting `AppRuntime` state.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "open_secondary_window (its production caller) is desktop-only -- android/wasm32 \
                  have no caller outside this module's own tests"
    )
)]
fn install_presentation_alongside(
    dispatcher: RealmDispatcher,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> Result<RealmDispatcher, InstallPresentationError> {
    let realm_id = dispatcher.address.realm_id;
    let owner_thread = dispatcher.owner_thread;
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.dispatched_realm_id.is_some() || state.iterating_all_realms {
            debug_assert!(
                false,
                "BUG: install_presentation_alongside called while a dispatch/hot-restart visit \
                 is in flight -- this path has no defer-to-idle queue yet (a named, stated gap, \
                 not a silent one); call only from outside any dispatched callback"
            );
            tracing::error!(
                ?realm_id,
                "rejecting install_presentation_alongside while a dispatch is in flight"
            );
            return Err(InstallPresentationError::DispatchInFlight);
        }
        if !state.realms.contains_key(&realm_id) {
            return Err(InstallPresentationError::RealmUnavailable);
        }
        // Authorization check (mirrors `dispatch_platform_realm`'s own):
        // the realm existing is not enough -- `dispatcher.address` itself
        // must still be a LIVE registered address. A realm survives its
        // sole non-primary presentation closing (a sibling keeps it
        // alive), so a caller could otherwise still hold a dispatcher for
        // that now-closed presentation and use it to authorize installing
        // yet another one alongside -- checked BEFORE borrowing
        // `state.realms` mutably below, exactly like `dispatch_platform_
        // realm` checks `state.registry` before `state.realms.get_mut`.
        if !state.registry.contains_address(dispatcher.address) {
            tracing::debug!(
                ?dispatcher,
                "rejecting install_presentation_alongside: the dispatcher's own presentation is \
                 no longer registered, even though its realm survives"
            );
            return Err(InstallPresentationError::StalePresentation);
        }
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence just checked above via contains_key");
        let Some(realm) = realm_slot.realm.as_mut() else {
            return Err(InstallPresentationError::RealmUnavailable);
        };
        // Assemble WITHOUT installing yet -- see this function's own doc
        // for why the ordering matters. `presentation` is dropped (no
        // forest membership, so nothing to roll back) if registration
        // below fails.
        let presentation = realm.assemble_presentation(std::sync::Arc::clone(window));
        let address = flui_foundation::PresentationAddress {
            realm_id,
            presentation_id: presentation.id(),
        };
        state.registry.try_register_window(window, address)?;
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence checked above, and nothing between here and there removed it");
        let realm = realm_slot
            .realm
            .as_mut()
            .expect("BUG: presence checked above, and nothing between here and there took it");
        realm.install_presentation(presentation);
        Ok(RealmDispatcher {
            owner_thread,
            address,
        })
    })
}

/// Uninstalls exactly one realm — tearing down a whole [`WindowPolicy::
/// SeparateRealms`]/`SharedRealm` group at once, unconditionally, regardless
/// of how many presentations it still hosts — without disturbing any other
/// hosted realm. Requests the removal through [`super::runtime::AppRuntime::
/// request_realm_uninstall`], so a request arriving mid-dispatch or
/// mid-hot-restart-visit defers to loop idle instead of mutating the
/// registry another operation is still walking.
///
/// No production embedder call site: an ordinary window closing always goes
/// through [`close_this_window`]/[`close_presentation`] instead, which
/// reduces to exactly this same effect only when the closing presentation is
/// its realm's sole one — calling this directly from a window's own close
/// handler would tear down an entire `SharedRealm` group out from under a
/// still-open sibling window, which is precisely the bug a prior revision of
/// this function's own caller had. Exercised directly by this module's own
/// tests (which construct scenarios `close_this_window` cannot, e.g. forcibly
/// tearing down a realm that still hosts more than one presentation, to pin
/// this function's own "whole group, unconditionally" contract in isolation).
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no production embedder call site -- an ordinary window close goes through \
                  close_this_window/close_presentation instead, which reduces to this same \
                  effect only for a realm's sole presentation; exercised by this module's own \
                  tests"
    )
)]
fn uninstall_platform_realm(realm_id: RealmId) {
    let removed = APP_RUNTIME.with(|slot| slot.borrow_mut().request_realm_uninstall(realm_id));
    // Destructors may re-enter platform/framework code — drop only after the
    // TLS borrow above has released.
    drop(removed);
}

/// Requests that one presentation be closed and removed from `dispatcher`'s
/// realm — a single window closing out of a realm that hosts more than one,
/// without tearing down the realm itself (contrast [`uninstall_platform_realm`],
/// which removes a whole realm). Request-shaped, like every other realm-map
/// mutation in this module: this function only enqueues
/// [`RealmTask::ClosePresentation`] and (if the realm is currently idle)
/// drives the drain loop that runs it — it never runs the six teardown
/// steps itself, and never returns anything about their outcome beyond
/// whether the request was accepted for this dispatcher (see
/// [`dispatch_platform_realm`]'s own `Err` variants).
///
/// The six steps run at Idle inside [`super::ui_realm::UiRealm::close_presentation_entered`],
/// which only [`dispatch_platform_realm`]'s own drain loop calls, and only
/// with the realm checked out as an owned local — so a dispose callback the
/// closed presentation runs mid-teardown sees the exact same
/// `dispatched_realm_id` TLS state (and therefore the exact same
/// install/uninstall deferral discipline) any other dispatched frame or
/// event callback sees. Production contract: async-at-Idle only — a caller
/// needing the six steps to have actually run before proceeding must drive
/// the dispatch loop to idle itself (a synchronous bypass would defeat the
/// whole point of routing this through the dispatch seam).
///
/// Production caller: [`close_this_window`] — the single `on_close` wiring
/// point every window this crate opens uses (`run_desktop`'s primary,
/// [`open_secondary_window`]'s new window under either [`WindowPolicy`]).
/// This is the SAME entry point regardless of whether `id` turns out to be
/// its realm's sole presentation (routes to a full realm uninstall above) or
/// one of several (removes just this one, siblings and realm survive) —
/// #555 closes with this slice; there is no further slice deferring this.
/// Also exercised directly by this module's own tests.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "close_this_window (its one production caller) is desktop-only -- \
                  android/wasm32 have no caller outside this module's own tests"
    )
)]
fn close_presentation(
    dispatcher: RealmDispatcher,
    id: flui_foundation::PresentationId,
) -> Result<(), RealmDispatchError> {
    dispatch_platform_realm(dispatcher, RealmTask::ClosePresentation(id))
}

/// Closes exactly the window `dispatcher` addresses — the single production
/// `on_close` wiring point for every window this crate opens (`run_desktop`'s
/// primary, both [`open_secondary_window`] policies). Routes through
/// [`close_presentation`], which correctly reduces to a full realm uninstall
/// when `dispatcher`'s presentation is its realm's ONLY one (a
/// [`WindowPolicy::SeparateRealms`] window, or the last surviving
/// presentation of a [`WindowPolicy::SharedRealm`] group), or removes just
/// that one presentation while its realm and any sibling presentation
/// survive otherwise — never [`uninstall_platform_realm`] directly, which
/// would tear down an ENTIRE `SharedRealm` group out from under a still-open
/// sibling window.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "its production callers (run_desktop, open_secondary_window) are desktop-only \
                  -- android/wasm32 have no caller outside this module's own tests"
    )
)]
fn close_this_window(dispatcher: RealmDispatcher) {
    if let Err(error) = close_presentation(dispatcher, dispatcher.address.presentation_id) {
        tracing::warn!(?dispatcher, ?error, "close_this_window: dispatch refused");
    }
}

#[cfg(not(target_os = "ios"))]
fn dispatch_platform_realm(
    dispatcher: RealmDispatcher,
    event: RealmTask,
) -> Result<(), RealmDispatchError> {
    if std::thread::current().id() != dispatcher.owner_thread {
        tracing::error!(?dispatcher, "rejecting realm callback on non-owner thread");
        return Err(RealmDispatchError::WrongThread);
    }
    let realm_id = dispatcher.address.realm_id;
    let checked_out = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Normative compare order (ADR-0037): realm first, then
        // presentation. `realm_id`/`presentation_id` mint from one shared
        // counter, so teardown+reinstall always changes both and the realm
        // check fires first on the common path.
        if state.realms.is_empty() {
            tracing::debug!(
                ?dispatcher,
                "dropping realm callback: no realm installed (not yet ready, or already torn down)"
            );
            return Err(RealmDispatchError::RealmUnavailable);
        }
        if !state.realms.contains_key(&realm_id) {
            tracing::debug!(
                ?dispatcher,
                "dropping realm callback: a newer realm replaced the one it was dispatched for"
            );
            return Err(RealmDispatchError::StaleRealm);
        }
        // Presentation check (issue #555's addressed-routing slice): membership in the SAME
        // `WindowRegistry` authority `dispatch_platform_realm`'s own
        // production window callbacks are resolved through, not equality
        // against `realm_slot.address` (which tracks only ONE presentation —
        // this realm's primary). A realm hosting N presentations has N live
        // addresses at once; a dispatcher naming any one of them, as long as
        // its exact address is still registered, is live -- one closed
        // (unregistered) since the dispatcher was minted, or a forged/mixed
        // address, is `StalePresentation` either way. Checked BEFORE
        // borrowing `realm_slot` mutably below (the registry is a sibling
        // field on the same `state`, so an immutable read here and a mutable
        // `realms` borrow next cannot overlap).
        if !state.registry.contains_address(dispatcher.address) {
            tracing::debug!(
                ?dispatcher,
                "dropping realm callback: presentation incarnation mismatch within the live realm"
            );
            return Err(RealmDispatchError::StalePresentation);
        }
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence just checked above via contains_key");
        // Same-realm reentrancy: this realm is already draining (mid its
        // own drain loop below) or checked out (by its own dispatch, or by
        // a `for_each_installed_realm` visit) -- always safe to enqueue and
        // return early; the ongoing drain loop, or the next legitimate
        // dispatch once the realm is restored, picks the event up.
        if realm_slot.draining || realm_slot.realm.is_none() {
            realm_slot
                .queue
                .push_back((dispatcher.address.presentation_id, event));
            return Ok(None);
        }
        // Nested cross-realm dispatch guard (issue #555), checked BEFORE
        // enqueuing `event` anywhere: reaching here means THIS realm is
        // neither draining nor checked out (both ruled out just above), so
        // a `dispatched_realm_id` naming a DIFFERENT realm can only mean a
        // nested dispatch was attempted while that other realm's task is
        // still running on this same thread — the one case
        // `request_realm_install`/`request_realm_uninstall`'s defer-to-idle
        // discipline exists to make structurally unreachable in production.
        // Checking this BEFORE the enqueue below is load-bearing, not
        // cosmetic: enqueuing `event` first and rejecting after would leave
        // it stuck in this realm's queue forever (nothing else ever removes
        // a rejected event), silently delivered to the next LEGITIMATE
        // dispatch instead of the genuine rejection this error reports.
        // Caught loudly in debug builds; release builds still refuse rather
        // than nest.
        if let Some(dispatched_realm_id) = state.dispatched_realm_id {
            debug_assert!(
                false,
                "BUG: nested cross-realm dispatch: realm {realm_id:?} dispatched while realm \
                 {dispatched_realm_id:?} is still checked out on this thread -- installs/\
                 uninstalls must defer to loop idle instead of nesting"
            );
            tracing::error!(
                ?realm_id,
                ?dispatched_realm_id,
                "rejecting nested cross-realm dispatch"
            );
            return Err(RealmDispatchError::NestedCrossRealmDispatchRejected);
        }
        let realm_slot = state
            .realms
            .get_mut(&realm_id)
            .expect("BUG: presence checked above");
        realm_slot
            .queue
            .push_back((dispatcher.address.presentation_id, event));
        let first = realm_slot
            .queue
            .pop_front()
            .expect("BUG: event was enqueued before starting realm dispatch");
        realm_slot.draining = true;
        let realm = realm_slot.realm.take();
        // Stash a clone of the checked-out realm's scheduler (and its
        // identity) BEFORE it leaves this slot: `installed_realm_phase`
        // (with_owner_platform's fence (c)) reads `dispatched_scheduler` as
        // its fallback whenever no OTHER realm's dispatch is in flight, which
        // is exactly the state this call is about to create for the entire
        // duration of the dispatched task below — otherwise the fence goes
        // blind for every real production frame, not merely when no realm is
        // installed at all. `UpdateScheduler::clone` is one `Arc::clone` (see
        // `flui-scheduler`'s single-`Arc` handle shape), not a second
        // scheduler.
        state.dispatched_scheduler = realm.as_ref().map(|realm| realm.scheduler().clone());
        state.dispatched_realm_id = Some(realm_id);
        Ok(realm.map(|realm| (realm, first)))
    })?;
    let Some((mut realm, first)) = checked_out else {
        return Ok(());
    };

    // Never hold the TLS RefCell borrow across user/platform callbacks. Catch
    // only to restore the host invariants; the original panic is resumed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut next = Some(first);
        while let Some((task_presentation_id, event)) = next {
            // `ClosePresentation` is matched out here, before it would
            // otherwise reach `RealmTask::run`'s `&UiRealm` receiver: closing
            // a presentation removes it from the forest (`&mut UiRealm`),
            // which is only available in this exact window -- `realm` sits
            // here as an owned local, checked out of `APP_RUNTIME` for the
            // whole dispatched task, so `&mut` falls out naturally instead of
            // needing interior mutability on the forest itself.
            match event {
                RealmTask::ClosePresentation(id) => {
                    if realm.is_sole_presentation(id) {
                        // Closing the realm's ONLY presentation IS closing
                        // the realm. Dispatch Detached FIRST, through this
                        // exact realm, before requesting the uninstall --
                        // shutdown must cancel any in-flight pointer
                        // sequence whose platform Up/Cancel will never
                        // arrive, and must notify lifecycle observers,
                        // before the realm and its `UpdateScheduler` are gone.
                        // This is the same reason `on_quit`'s own Detached
                        // dispatch exists (`run_desktop`'s bootstrap),
                        // generalized to per-realm teardown instead of only
                        // process-wide quit: a realm closing because its one
                        // window closed is exactly as "detached" as one
                        // closing because the whole process quit, and
                        // `on_quit`'s own dispatch now frequently finds this
                        // realm already gone (a harmless, traced no-op --
                        // see that callback's doc). Uses `PlatformToUi::
                        // Lifecycle(..).run` directly (the same private
                        // helper an ordinary `RealmTask::Event(PlatformToUi::
                        // Lifecycle(..))` dispatches through below) rather
                        // than re-queuing another task, since `realm` is
                        // already the exact owned local that method needs.
                        PlatformToUi::Lifecycle(AppLifecycleState::Detached).run(&realm, id);

                        // Closing the realm's ONLY presentation IS closing
                        // the realm -- routing it through
                        // close_presentation_entered would leave an empty
                        // forest behind (every other method on this realm,
                        // starting with `primary()`, assumes one always
                        // exists). Request a full realm uninstall through
                        // the SAME deferral machinery every other
                        // realm-map mutation already uses:
                        // `dispatched_realm_id` is `Some` for this whole
                        // checkout, so this defers to `dispatch_platform_
                        // realm`'s own tail (below), which runs
                        // `apply_uninstall` only after `realm` is restored
                        // to its slot -- `apply_uninstall` already removes
                        // every one of this realm's `WindowRegistry`
                        // entries before dropping the `RealmSlot` (see its
                        // own doc), so step 1 is covered by the SAME path
                        // a whole-realm close always used.
                        let displaced = APP_RUNTIME
                            .with(|slot| slot.borrow_mut().request_realm_uninstall(realm_id));
                        drop(displaced);
                    } else {
                        // Step 1: unregister exactly THIS presentation's own
                        // window mapping -- never a sibling's, and never
                        // every window this realm owns (`WindowRegistry::
                        // remove_realm` would be wrong here). `UiRealm`
                        // itself has no access to the registry (AppRuntime
                        // is its single authority, ADR-0037 §2), so this
                        // runs here, in the one caller that does, before
                        // handing off to close_presentation_entered's
                        // steps 2-6. Without this, a stale platform event
                        // for the closed presentation's original window
                        // would still resolve to its now-dead
                        // PresentationId instead of being refused.
                        let address = flui_foundation::PresentationAddress {
                            realm_id,
                            presentation_id: id,
                        };
                        let unregistered = APP_RUNTIME
                            .with(|slot| slot.borrow_mut().registry.remove_presentation(address));
                        drop(unregistered);

                        // Re-stamp this realm's tracked routable address
                        // (`RealmSlot::address`) to the surviving primary
                        // BEFORE running `id`'s own teardown/dispose hooks
                        // -- not after. `close_presentation_entered`'s step
                        // 2-3 (below) can run a dispose hook that re-enters
                        // this exact function with a dispatcher still
                        // bearing `id`; ordering the re-stamp first means
                        // that reentrant dispatch's `StalePresentation`
                        // check (at the top of this function) already
                        // compares against the NEW primary and correctly
                        // refuses it right there. Re-stamping AFTER
                        // disposal instead would leave a window where that
                        // same reentrant dispatch still compares equal
                        // (both sides still `id`), gets ACCEPTED by the
                        // stale check, and is merely enqueued behind the
                        // same-realm reentrancy guard -- only to run a
                        // moment later, in this very drain loop, against
                        // whatever survives the close: silently
                        // misaddressed rather than refused. See
                        // `UiRealm::primary_id_excluding`'s own doc for why
                        // this is computable before the removal happens.
                        // `None` is unreachable here: this branch runs only
                        // when `is_sole_presentation(id)` was `false` above,
                        // so at least one other presentation always exists.
                        if let Some(surviving_primary_id) = realm.primary_id_excluding(id) {
                            APP_RUNTIME.with(|slot| {
                                if let Some(realm_slot) =
                                    slot.borrow_mut().realms.get_mut(&realm_id)
                                {
                                    realm_slot.address.presentation_id = surviving_primary_id;
                                }
                            });
                        }

                        realm.close_presentation_entered(id);
                    }
                }
                other => realm.enter(|realm| other.run(realm, task_presentation_id)),
            }
            next = APP_RUNTIME.with(|slot| {
                slot.borrow_mut()
                    .realms
                    .get_mut(&realm_id)
                    .and_then(|realm_slot| realm_slot.queue.pop_front())
            });
        }
    }));
    let removed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // The slot may be gone entirely if a NESTED, non-dispatch teardown
        // (e.g. `teardown_platform_realm`'s full-registry clear, called
        // reentrantly from inside the just-run task) already removed it —
        // that dropped this realm's queue/applier already, so there is
        // nothing left to restore; just let `realm` fall out of scope below.
        if let Some(realm_slot) = state.realms.get_mut(&realm_id) {
            realm_slot.realm = Some(realm);
            realm_slot.draining = false;
        }
        // Cleared unconditionally in this same restore block, which runs
        // whether or not the dispatched task above panicked (the panic, if
        // any, is only resumed after this restore completes below) — the
        // fence-(c) fallback must not survive past the dispatch it was
        // stashed for.
        state.dispatched_scheduler = None;
        state.dispatched_realm_id = None;
        // Applies any realm-map mutation this realm's own task requested
        // (e.g. a dispose callback uninstalling itself, or a frame callback
        // installing a second realm) — deferred above precisely because
        // `dispatched_realm_id` was `Some` for the whole task, and safe to
        // apply now that it is cleared.
        state.drain_pending_realm_mutations()
    });
    // Destructors may re-enter platform/framework code — drop only after the
    // TLS borrow above has released.
    drop(removed);
    // Applies any `open_secondary_window` Pending-arm completion this
    // realm's own task resolved (see `drain_pending_secondary_window_
    // completions`'s own doc for why this must run only now, after the
    // checkout state above is cleared) — same "drain regardless of panic"
    // discipline as `drain_pending_realm_mutations` above, for the same
    // reason: a resolved secondary window should not sit unwired forever
    // just because this dispatch's own task panicked for an unrelated
    // reason. This function itself (`dispatch_platform_realm`) compiles on
    // every backend (only `not(target_os = "ios")`), but the drain helper
    // and everything it touches exist only on the desktop-only
    // `open_secondary_window` family's own cfg -- gate the call site to
    // match, not the function.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    drain_pending_secondary_window_completions();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
    Ok(())
}

/// Hot-restart's own iteration primitive (issue #555): visits every
/// currently-installed realm, in mount order, running `f` against each one
/// OUTSIDE the `APP_RUNTIME` borrow — the same checkout/restore discipline
/// [`dispatch_platform_realm`] uses for a single addressed realm.
///
/// What `f` may safely do, precisely (not "any `APP_RUNTIME`-touching
/// function" — the visited realm counts as dispatched, see below, so the
/// same restrictions a dispatched task's own closure has apply here too):
/// dispatch back to the SAME realm being visited (enqueues via the ordinary
/// same-realm reentrant path, never recurses); request a realm-map
/// install/uninstall (defers rather than applies immediately — see
/// [`super::runtime::AppRuntime::request_realm_install`]'s own doc — applied
/// only once every realm has been visited, so the set of realms visited
/// never shifts mid-iteration); or call [`super::runtime::AppRuntime::should_exit`]
/// (also defers its own drain while this visit is in flight, for the same
/// reason). Dispatching to a DIFFERENT, sibling realm is NOT safe: it now
/// trips `dispatch_platform_realm`'s nested-cross-realm-dispatch guard (see
/// the next paragraph), a deliberate behavior change, not an oversight.
///
/// Each visited realm is ALSO stashed into `dispatched_scheduler`/
/// `dispatched_realm_id` for the duration of its own call to `f` — the same
/// fields `dispatch_platform_realm` stashes for a dispatched task — so
/// `with_owner_platform`'s fence (c) stays able to see the visited realm's
/// own scheduler phase for the whole time it sits checked out of `realms`,
/// exactly as it does for a real dispatch. A side effect of that stash is
/// exactly the restriction stated above: `dispatch_platform_realm` treats a
/// visited realm as "checked out" indistinguishably from a dispatched one,
/// so a nested dispatch to a sibling realm from inside `f` is rejected by
/// the same nested-cross-realm-dispatch guard a nested dispatch from inside
/// another dispatch would be.
///
/// A panic inside `f` is caught, not propagated past this function's own
/// cleanup: the visited realm is restored to its slot, `iterating_all_realms`
/// is cleared, and any mutation deferred during the visit (including one
/// requested by the very call that panicked) is drained, all BEFORE the
/// panic resumes. Skipping any of that on a panicking visit would strand the
/// visited realm's slot at `realm: None` forever and wedge
/// `iterating_all_realms` at `true` forever — which, after
/// `drain_pending_realm_mutations`'s own guard against draining while a
/// visit is nominally still in flight, would silently defer every future
/// realm-map mutation for the rest of the process.
///
/// No production driver calls this yet — hot-restart's real trigger (the
/// `flui-hot-reload` file-watcher path) still only ever polls the single
/// dispatched realm through its own frame callback; a driver that visits
/// every hosted realm is this issue's follow-up. Exercised directly by this
/// module's own tests in the meantime.
#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "design-for-N hot-restart iteration primitive; no production driver visits \
                  every hosted realm yet -- exercised by this module's own tests"
    )
)]
fn for_each_installed_realm(mut f: impl FnMut(&super::ui_realm::UiRealm)) {
    let ids = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        debug_assert!(
            !state.iterating_all_realms,
            "BUG: reentrant for_each_installed_realm"
        );
        state.iterating_all_realms = true;
        state.realms.keys()
    });

    let mut panic_payload = None;
    for id in ids {
        let realm = APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm = state
                .realms
                .get_mut(&id)
                .and_then(|realm_slot| realm_slot.realm.take())?;
            // Mirrors `dispatch_platform_realm`'s own checkout stash: while
            // `realm` sits outside `realms` for the extent of `f` below,
            // fence (c) must still be able to read ITS phase, not go blind.
            state.dispatched_scheduler = Some(realm.scheduler().clone());
            state.dispatched_realm_id = Some(id);
            Some(realm)
        });
        let Some(realm) = realm else {
            // Removed by an earlier realm's own visit before this iteration
            // reached it -- unreachable under the defer-to-idle discipline
            // (a mutation requested mid-visit only applies AFTER the whole
            // visit completes), kept as a defensive skip rather than an
            // `expect`.
            continue;
        };

        // Never hold the TLS RefCell borrow across `f` -- the same
        // discipline `dispatch_platform_realm` follows for a dispatched
        // task. Catch only to restore the checked-out realm and this
        // visit's own bookkeeping; the original panic resumes once every
        // realm has been handled (visited, or left untouched after a
        // panic stops the walk).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&realm)));
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            if let Some(realm_slot) = state.realms.get_mut(&id) {
                realm_slot.realm = Some(realm);
            }
            state.dispatched_scheduler = None;
            state.dispatched_realm_id = None;
        });
        if let Err(payload) = result {
            panic_payload = Some(payload);
            break;
        }
    }

    let removed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.iterating_all_realms = false;
        state.drain_pending_realm_mutations()
    });
    drop(removed);
    // Same rationale as `dispatch_platform_realm`'s own tail: a visited
    // realm's frame callback may have resolved an `open_secondary_window`
    // Pending completion via `UpdateScheduler::drive_async_tasks`, which cannot
    // complete mid-visit for the same reason it cannot complete
    // mid-dispatch (`iterating_all_realms` holds this thread's checkout
    // state just as `dispatched_realm_id` does). This function itself
    // compiles on every backend (only `not(target_os = "ios")`) -- gate the
    // call site to the desktop-only `open_secondary_window` family's own
    // cfg, not the function.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    drain_pending_secondary_window_completions();

    if let Some(payload) = panic_payload {
        std::panic::resume_unwind(payload);
    }
}

/// Drains the per-frame owner-inbox commands and reports whether the drain
/// itself asked for a redraw.
///
/// Every platform's frame callback must call this exactly once per wake, at
/// the Idle frame boundary — before the dirty gate, and before any
/// early-return fast path a platform's frame callback takes (e.g. Android's
/// hot-reload plugin scene) — never inside the frame transaction below.
/// Running it unconditionally on every wake is what keeps
/// `UiCommandSender`'s bounded inbox draining: a wake that skips the drain
/// lets the inbox fill until it hard-errors, and a coalesced redraw request
/// that nothing consumes never wakes the loop again (`take_redraw_request`
/// only flips back to `false` once observed here).
#[cfg(not(target_os = "ios"))]
fn drain_owner_inbox(realm: &super::ui_realm::UiRealm) -> bool {
    let report = realm.drain_commands();
    if report != super::ui_realm::DrainReport::default() {
        tracing::trace!(?report, "owner inbox drained");
    }
    realm.take_redraw_request()
}

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
fn teardown_platform_realm() {
    let realms = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Registry removal first (ADR-0037 §2): stop new routing before the
        // queued old-generation events below are dropped, and before the
        // registry is cleared. The teardown real read: assert the removed
        // entries include the address EACH realm installed — `remove_realm`
        // removes every window mapped to that realm, not just the first, so
        // a one-realm-many-windows install still leaves nothing behind.
        //
        // Every hosted realm tears down here, not just one: this runs from
        // `run_desktop`/`run_android` after their respective
        // `platform.run(...)` returns, i.e. the WHOLE loop is exiting, so
        // every realm this thread ever hosted goes with it.
        let realms = state.realms.clear();
        for (id, realm_slot) in &realms {
            let removed = state.registry.remove_realm(*id);
            debug_assert!(
                removed
                    .iter()
                    .any(|(_, removed_address)| *removed_address == realm_slot.address),
                "BUG: window_registry teardown read did not include the installed address"
            );
        }
        state.owner_thread = None;
        state.dispatched_scheduler = None;
        state.dispatched_realm_id = None;
        state.iterating_all_realms = false;
        debug_assert!(
            !state.has_pending_realm_mutations(),
            "BUG: realm-map mutations still pending at full loop-exit teardown -- \
             dispatch_platform_realm and for_each_installed_realm must drain \
             unconditionally in their own tails"
        );
        realms
    });
    // Destructors may re-enter platform/framework code. Drop only after the
    // TLS borrow and incarnation identity have been released.
    drop(realms);

    // ADR-0034's install/teardown symmetry: the event loop has exited (this
    // runs from both `run_desktop` and `run_android`, after their respective
    // `platform.run(...)` returns), so drop the platform clipboard now rather
    // than let a live platform resource (arboard on X11 owns a live X11
    // connection) sit pinned behind `AppRuntime` for the rest of the
    // process's life. `Drop for AppRuntime` is the last-resort third clear
    // if this explicit path is ever skipped (a panic mid-teardown, for
    // instance) — see that impl's doc.
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state.clear_platform_clipboard();
        state.clear_redraw_window();
    });
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod realm_dispatch_tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use flui_interaction::{
        HitTestResult,
        events::{PointerType, make_down_event},
    };
    use flui_platform::traits::PlatformInput;
    use flui_types::geometry::{Offset, Pixels};

    use super::*;

    static_assertions::assert_impl_all!(PlatformToUi: Send);

    fn down_input(offset: f32) -> PlatformInput {
        PlatformInput::Pointer(make_down_event(
            Offset::new(Pixels(offset), Pixels(offset)),
            PointerType::Mouse,
        ))
    }

    fn test_window() -> std::sync::Arc<dyn flui_platform::traits::PlatformWindow> {
        flui_platform::headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window")
    }

    fn install_test_realm() -> RealmDispatcher {
        install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &test_window())
    }

    /// A second realm installed on the same thread (hot-restart; sequential
    /// test realms) must not inherit a PRIOR realm's stale `(visible,
    /// focused)` window signals — every backend's window starts visible and
    /// focused, so a fresh realm's derivation must start from that same
    /// baseline.
    ///
    /// If reverted: remove the `state.visible = true; state.focused = true;`
    /// reset from `install_platform_realm` and this fails — the second
    /// realm reads `visible == false` left behind by the first.
    #[test]
    fn install_platform_realm_resets_stale_visible_focused_from_a_prior_realm() {
        install_test_realm();
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            state.visible = false;
            state.focused = false;
        });
        teardown_platform_realm();

        install_test_realm();
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            assert!(
                state.visible,
                "a realm installed on this thread must start visible, not inherit a stale \
                 value from a prior realm"
            );
            assert!(
                state.focused,
                "a realm installed on this thread must start focused, not inherit a stale \
                 value from a prior realm"
            );
        });
        teardown_platform_realm();
    }

    /// Panic-recovery reinstall: `install_platform_realm` called again while
    /// a realm from a prior incarnation is still installed (a mid-`on_ready`
    /// failure that never reached `teardown_platform_realm`). The displaced
    /// realm's queue must never leak stale-incarnation events into the new
    /// realm, the displaced realm/queue/applier must drop only after the TLS
    /// borrow releases (a reentrant dispatch triggered by that drop must see
    /// a clean, already-released borrow — never a double-borrow panic), and
    /// `draining` must not be left stuck from whatever state the displaced
    /// realm was in.
    ///
    /// If reverted: without `mem::take`-ing the queue before overwriting
    /// `state.realm`, the pre-existing stale event is delivered to the NEW
    /// realm FIFO-first and this fails on the `delivered` assertion.
    #[test]
    fn reinstall_without_teardown_drops_the_displaced_realm_and_queue_outside_the_borrow() {
        struct ReenterOnDrop {
            dispatcher: RealmDispatcher,
            result: Rc<RefCell<Option<Result<(), RealmDispatchError>>>>,
        }

        impl Drop for ReenterOnDrop {
            fn drop(&mut self) {
                let result =
                    dispatch_platform_realm(self.dispatcher, RealmTask::Frame(Box::new(|_| {})));
                *self.result.borrow_mut() = Some(result);
            }
        }

        let dispatcher_a = install_test_realm();
        let reentry_result = Rc::new(RefCell::new(None));
        let reentry_result_in_probe = Rc::clone(&reentry_result);
        let probe = ReenterOnDrop {
            dispatcher: dispatcher_a,
            result: reentry_result_in_probe,
        };

        let delivered = Rc::new(RefCell::new(false));
        let delivered_in_event = Rc::clone(&delivered);

        // Enqueue directly (not through `dispatch_platform_realm`, which
        // would drain immediately) — this is the stale-incarnation queue
        // state a mid-`on_ready` panic can leave behind before bootstrap
        // retries `install_platform_realm` without ever calling
        // `teardown_platform_realm`. Also force `draining = true`, matching
        // a realm that was mid-drain when the panic hit.
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm_slot = state
                .realms
                .get_mut(&dispatcher_a.address.realm_id)
                .expect("dispatcher_a's realm is installed above");
            let stamp = dispatcher_a.address.presentation_id;
            realm_slot.queue.push_back((
                stamp,
                RealmTask::Frame(Box::new(move |_| {
                    *delivered_in_event.borrow_mut() = true;
                })),
            ));
            realm_slot
                .queue
                .push_back((stamp, RealmTask::Frame(Box::new(move |_| drop(probe)))));
            realm_slot.draining = true;
        });

        // The panic-recovery reinstall itself: no teardown in between.
        let dispatcher_b = install_test_realm();

        assert!(
            !*delivered.borrow(),
            "a queued event from the displaced incarnation must never reach the new realm"
        );
        assert_eq!(
            *reentry_result.borrow(),
            Some(Err(RealmDispatchError::StaleRealm)),
            "the displaced realm/queue must drop only after install_platform_realm's TLS \
             borrow releases — a drop still inside that borrow would panic this reentrant \
             dispatch with a double-borrow instead of returning a clean Err"
        );

        let new_realm_ran = Rc::new(RefCell::new(false));
        let new_realm_ran_in_event = Rc::clone(&new_realm_ran);
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(move |_| {
                *new_realm_ran_in_event.borrow_mut() = true;
            })),
        )
        .expect("the new realm dispatches normally");
        assert!(
            *new_realm_ran.borrow(),
            "draining must not be left stuck from the displaced incarnation — the new \
             realm must actually drain, not just enqueue forever"
        );
        teardown_platform_realm();
    }

    /// `install_platform_realm` only ever touches the realm-facing fields
    /// (`realm`, `queue`, `owner_thread`, `address`, `surface_applier`,
    /// `visible`, `focused`) — `owner_platform` is a separate, loop-scoped
    /// concern that must survive both branches: a fresh install, and a
    /// replace-without-teardown reinstall that displaces a realm never torn
    /// down (the same panic-recovery path
    /// `reinstall_without_teardown_drops_the_displaced_realm_and_queue_outside_the_borrow`
    /// exercises above).
    #[test]
    fn install_platform_realm_never_touches_owner_platform() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let result = platform.run(Box::new(|owner| {
            install_owner_platform(owner);
            assert!(
                with_owner_platform(|_| ()).is_some(),
                "owner_platform must be installed before the first realm install"
            );

            // Fresh-install branch.
            install_test_realm();
            assert!(
                with_owner_platform(|_| ()).is_some(),
                "a fresh realm install must not clear owner_platform"
            );

            // Replace-without-teardown branch: install a second realm while
            // the first is still live, without an intervening
            // `teardown_platform_realm` (the panic-recovery path).
            install_test_realm();
            assert!(
                with_owner_platform(|_| ()).is_some(),
                "a replace-without-teardown reinstall must not clear owner_platform"
            );

            teardown_platform_realm();
            Ok(())
        }));
        assert!(result.is_ok(), "on_ready returns Ok here");
    }

    /// A panic-recovery reinstall under a DIFFERENT native window must
    /// remove the displaced realm's own window mapping from the registry —
    /// not just leave it behind alongside the new one.
    ///
    /// Both windows are opened from the *same* headless platform instance:
    /// `HeadlessPlatform` mints window ids from its own instance-local
    /// counter, so two windows from two separate `headless_platform()` calls
    /// would alias the same id instead of differing — one instance, two
    /// `open_window` calls, is what actually produces two distinct windows.
    ///
    /// If reverted: skip the registry cleanup for the displaced realm in
    /// `install_platform_realm` and this fails — the first window still
    /// resolves, and the registry holds two entries instead of one.
    #[test]
    fn reinstall_with_a_different_window_removes_the_old_windows_registry_mapping() {
        let platform = flui_platform::headless_platform();
        let first_window = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create the first test window");
        let second_window = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create the second test window");
        let first_window_id = first_window.id();
        let second_window_id = second_window.id();
        assert_ne!(
            first_window_id, second_window_id,
            "the two windows must have distinct ids for this test to mean anything"
        );

        let _first_dispatcher =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &first_window);
        // The panic-recovery reinstall itself, under the second window: no
        // teardown in between.
        let _second_dispatcher =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &second_window);

        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            assert_eq!(
                state.registry.resolve(first_window_id),
                None,
                "the displaced realm's old window mapping must be removed on reinstall"
            );
            assert!(
                state.registry.resolve(second_window_id).is_some(),
                "the new window must resolve to the new realm's address"
            );
            assert_eq!(
                state.registry.len(),
                1,
                "only the new window's mapping may remain after the reinstall"
            );
        });
        teardown_platform_realm();
    }

    #[test]
    fn detached_realm_event_cancels_an_interrupted_pointer_sequence() {
        let dispatcher = install_test_realm();
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        )
        .expect("test realm resumes");
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                let down =
                    make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Touch);
                realm
                    .gestures()
                    .handle_pointer_event(&down, |_| HitTestResult::new());
                assert_eq!(realm.gestures().active_pointer_count(), 1);
            })),
        )
        .expect("pointer sequence starts");

        dispatch_platform_realm(
            dispatcher,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Detached)),
        )
        .expect("Detached lifecycle dispatches through the realm");
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(realm.gestures().active_pointer_count(), 0);
                assert_eq!(realm.gestures().active_resampler_count(), 0);
                assert_eq!(realm.gestures().pending_move_count(), 0);
                assert!(realm.gestures().arena().is_empty());
            })),
        )
        .expect("clean state remains observable");

        dispatch_platform_realm(
            dispatcher,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        )
        .expect("test realm lifecycle restores");
        teardown_platform_realm();
    }

    #[test]
    fn reentrant_frame_event_is_queued_fifo() {
        let dispatcher = install_test_realm();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_| {
                outer.borrow_mut().push(1);
                let nested = Rc::clone(&outer);
                dispatch_platform_realm(
                    dispatcher,
                    RealmTask::Frame(Box::new(move |_| {
                        nested.borrow_mut().push(3);
                    })),
                )
                .expect("nested event queues");
                outer.borrow_mut().push(2);
            })),
        )
        .expect("outer event dispatches");
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn late_event_never_crosses_realm_incarnations() {
        let stale = install_test_realm();
        let removed =
            APP_RUNTIME.with(|slot| slot.borrow_mut().realms.remove(&stale.address.realm_id));
        drop(removed);
        assert_eq!(
            dispatch_platform_realm(stale, RealmTask::Frame(Box::new(|_| {}))),
            Err(RealmDispatchError::RealmUnavailable)
        );

        let current = install_test_realm();
        assert_eq!(
            dispatch_platform_realm(stale, RealmTask::Frame(Box::new(|_| {}))),
            Err(RealmDispatchError::StaleRealm)
        );
        dispatch_platform_realm(current, RealmTask::Frame(Box::new(|_| {})))
            .expect("current incarnation dispatches");
    }

    /// The common path (ADR-0037 compare order, realm first): teardown +
    /// reinstall mints both a fresh `RealmId` and a fresh `PresentationId`
    /// from the same shared counter, so a stale dispatcher's realm half
    /// never matches — the presentation half is never even compared.
    ///
    /// If reverted: remove the realm-id compare from `dispatch_platform_realm`
    /// and this fails — the stale dispatcher's input reaches the new realm.
    #[test]
    fn stale_realm_dispatch_is_dropped() {
        let stale = install_test_realm();
        teardown_platform_realm();
        let _current = install_test_realm();

        assert_eq!(
            dispatch_platform_realm(
                stale,
                RealmTask::Event(PlatformToUi::Input(down_input(1.0))),
            ),
            Err(RealmDispatchError::StaleRealm)
        );
        teardown_platform_realm();
    }

    /// The design-for-N path (reachable today only via a forged/mixed
    /// address, since realm/presentation generations always advance
    /// together): a dispatcher whose realm half matches the live realm but
    /// whose presentation half names a different incarnation must be
    /// dropped as `StalePresentation`, and the live realm's own gesture
    /// state must be completely untouched by the attempt.
    ///
    /// If reverted: remove the presentation-id compare from
    /// `dispatch_platform_realm` and this fails — the forged dispatcher's
    /// input reaches the live realm's arena.
    #[test]
    fn stale_presentation_with_live_realm_is_dropped() {
        let live = install_test_realm();
        let live_generation = live.address.presentation_id.generation();
        let forged_generation = std::num::NonZeroU32::new(live_generation.get() + 1)
            .expect("live_generation + 1 is nonzero");
        let forged = RealmDispatcher {
            owner_thread: live.owner_thread,
            address: flui_foundation::PresentationAddress {
                realm_id: live.address.realm_id,
                presentation_id: flui_foundation::PresentationId::new_gen(0, forged_generation),
            },
        };

        assert_eq!(
            dispatch_platform_realm(
                forged,
                RealmTask::Event(PlatformToUi::Input(down_input(1.0))),
            ),
            Err(RealmDispatchError::StalePresentation)
        );

        dispatch_platform_realm(
            live,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    0,
                    "the live realm's gesture state must be untouched by a dropped forged dispatch"
                );
            })),
        )
        .expect("the live dispatcher still dispatches");
        teardown_platform_realm();
    }

    /// The AC-named teardown test: events queued before teardown for a
    /// removed presentation must never deliver, and the surface applier
    /// (cleared at the same teardown point) must not fire for a queued
    /// resize either.
    ///
    /// If reverted: have `teardown_platform_realm` run the queue instead of
    /// dropping it, and both assertions below fail.
    #[test]
    fn queued_events_for_a_removed_presentation_never_deliver() {
        let dispatcher = install_test_realm();
        let delivered = Rc::new(RefCell::new(false));
        let delivered_in_event = Rc::clone(&delivered);
        let applier_invoked = Rc::new(RefCell::new(false));
        let applier_invoked_in_closure = Rc::clone(&applier_invoked);
        install_surface_applier(dispatcher.address.realm_id, move |_size, _scale_factor| {
            *applier_invoked_in_closure.borrow_mut() = true;
        });

        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm_slot = state
                .realms
                .get_mut(&dispatcher.address.realm_id)
                .expect("just installed above");
            let stamp = dispatcher.address.presentation_id;
            realm_slot.queue.push_back((
                stamp,
                RealmTask::Frame(Box::new(move |_| {
                    *delivered_in_event.borrow_mut() = true;
                })),
            ));
            realm_slot.queue.push_back((
                stamp,
                RealmTask::Event(PlatformToUi::Resized {
                    size: flui_types::Size::new(
                        flui_types::geometry::px(100.0),
                        flui_types::geometry::px(100.0),
                    ),
                    scale_factor: 1.0,
                }),
            ));
        });

        teardown_platform_realm();
        let _new_realm = install_test_realm();

        assert!(
            !*delivered.borrow(),
            "a queued event for a removed presentation must never deliver"
        );
        assert!(
            !*applier_invoked.borrow(),
            "the surface applier must not fire for a queued resize after teardown"
        );
        teardown_platform_realm();
    }

    /// Borrow-discipline test: a `Resized` event dispatched while the
    /// registration-lifetime applier slot is empty (no applier installed
    /// yet, or already torn down) must skip with a trace rather than
    /// unwrap/panic — the take/call/restore protocol's `None` arm.
    ///
    /// A genuinely *nested* re-entrant call (the applier's own call
    /// triggering another `Resized` dispatch before it returns) cannot
    /// observe an empty slot here: `dispatch_platform_realm` always defers a
    /// call made while `draining` to the FIFO queue rather than running it
    /// synchronously, and by the time that queued event is drained the
    /// outer call has already restored the slot. The `None` arm exists for
    /// the case this test exercises directly: a `Resized` event reaching
    /// [`PlatformToUi::run`] before [`install_surface_applier`] has run (or
    /// after it has been cleared), never for synchronous reentrancy.
    ///
    /// If reverted: replace the `None` arm's trace-and-skip with
    /// `.expect("applier installed")` and this panics instead of returning
    /// `Ok`.
    #[test]
    fn resized_with_no_applier_installed_skips_instead_of_panicking() {
        let dispatcher = install_test_realm();
        // Deliberately no `install_surface_applier` call: the slot starts
        // (and stays) empty.

        let result = dispatch_platform_realm(
            dispatcher,
            RealmTask::Event(PlatformToUi::Resized {
                size: flui_types::Size::new(
                    flui_types::geometry::px(20.0),
                    flui_types::geometry::px(20.0),
                ),
                scale_factor: 1.0,
            }),
        );

        assert!(
            result.is_ok(),
            "a Resized event with no applier installed must not panic; it \
             coalesces onto the next real applier install instead"
        );
        teardown_platform_realm();
    }

    #[test]
    fn panic_restores_dispatch_host_for_next_event() {
        let dispatcher = install_test_realm();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = dispatch_platform_realm(
                dispatcher,
                RealmTask::Frame(Box::new(|_| panic!("test panic"))),
            );
        }));
        assert!(panic.is_err());

        let ran = Rc::new(RefCell::new(false));
        let ran_in_event = Rc::clone(&ran);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_| {
                *ran_in_event.borrow_mut() = true;
            })),
        )
        .expect("host restored");
        assert!(*ran.borrow());
    }

    /// A panic inside the surface applier's own call (caught by
    /// `dispatch_platform_realm`'s outer `catch_unwind`, same as any other
    /// panicking event) must not permanently strand resizing: the
    /// `SurfaceApplierRestoreGuard` restores the applier into the TLS slot
    /// during the unwinding drop, so the next `Resized` still reaches it.
    ///
    /// If reverted: revert to restoring the applier only after a successful
    /// call (no drop guard) and this fails — the second `Resized` silently
    /// coalesces at the `None` arm instead of calling the applier again.
    #[test]
    fn surface_applier_panic_is_caught_and_the_applier_still_applies_next_time() {
        let dispatcher = install_test_realm();
        let calls = Rc::new(RefCell::new(0));
        let calls_in_closure = Rc::clone(&calls);
        install_surface_applier(dispatcher.address.realm_id, move |_size, _scale_factor| {
            *calls_in_closure.borrow_mut() += 1;
            assert_ne!(
                *calls_in_closure.borrow(),
                1,
                "surface applier panics on its first call (simulated backend failure)"
            );
        });

        let resize_event = |side: f32| {
            RealmTask::Event(PlatformToUi::Resized {
                size: flui_types::Size::new(
                    flui_types::geometry::px(side),
                    flui_types::geometry::px(side),
                ),
                scale_factor: 1.0,
            })
        };

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = dispatch_platform_realm(dispatcher, resize_event(20.0));
        }));
        assert!(
            panic.is_err(),
            "the first Resized's applier call must panic"
        );

        dispatch_platform_realm(dispatcher, resize_event(30.0))
            .expect("dispatch after the caught panic still succeeds");

        assert_eq!(
            *calls.borrow(),
            2,
            "the second Resized must still reach the applier — a panic on the first call \
             must not permanently strand resizing"
        );
        teardown_platform_realm();
    }

    #[test]
    fn callback_on_wrong_thread_is_rejected() {
        let dispatcher = install_test_realm();
        let result = std::thread::spawn(move || {
            dispatch_platform_realm(dispatcher, RealmTask::Frame(Box::new(|_| {})))
        })
        .join()
        .expect("worker test thread");
        assert_eq!(result, Err(RealmDispatchError::WrongThread));
    }

    #[test]
    fn nested_resize_and_window_focus_wait_until_frame_returns() {
        let dispatcher = install_test_realm();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        let applier_calls = Rc::clone(&order);
        install_surface_applier(dispatcher.address.realm_id, move |_size, _scale_factor| {
            applier_calls.borrow_mut().push(3);
        });
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_| {
                outer.borrow_mut().push(1);
                dispatch_platform_realm(
                    dispatcher,
                    RealmTask::Event(PlatformToUi::WindowFocus(true)),
                )
                .expect("window focus queues");
                dispatch_platform_realm(
                    dispatcher,
                    RealmTask::Event(PlatformToUi::Resized {
                        size: flui_types::Size::new(
                            flui_types::geometry::px(640.0),
                            flui_types::geometry::px(480.0),
                        ),
                        scale_factor: 2.0,
                    }),
                )
                .expect("resize queues");
                outer.borrow_mut().push(2);
            })),
        )
        .expect("frame dispatches");
        // Two different `PlatformToUi` variants nested inside a `Frame` still
        // queue FIFO rather than running immediately — the property
        // `reentrant_frame_event_is_queued_fifo` proves for same-variant
        // nesting, this proves it holds across variant types too.
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn teardown_drops_queued_destructors_outside_tls_borrow() {
        struct ReenterOnDrop {
            dispatcher: RealmDispatcher,
            dropped: Rc<RefCell<bool>>,
        }

        impl Drop for ReenterOnDrop {
            fn drop(&mut self) {
                let result =
                    dispatch_platform_realm(self.dispatcher, RealmTask::Frame(Box::new(|_| {})));
                assert_eq!(result, Err(RealmDispatchError::RealmUnavailable));
                *self.dropped.borrow_mut() = true;
            }
        }

        let dispatcher = install_test_realm();
        let dropped = Rc::new(RefCell::new(false));
        let probe = ReenterOnDrop {
            dispatcher,
            dropped: Rc::clone(&dropped),
        };
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .realms
                .get_mut(&dispatcher.address.realm_id)
                .expect("just installed above")
                .queue
                .push_back((
                    dispatcher.address.presentation_id,
                    RealmTask::Frame(Box::new(move |_| drop(probe))),
                ));
        });
        teardown_platform_realm();
        assert!(*dropped.borrow());
    }

    #[cfg(feature = "hot-reload")]
    #[test]
    fn old_registered_hot_reload_hook_cannot_touch_recreated_realm() {
        use flui_hot_reload::{register_request_rebuild, request_rebuild};

        use crate::app::hot_reload::queued_hot_reload_hook;

        let runtime_a = super::super::ui_realm::UiRealm::for_test();
        let sender_a = runtime_a.command_sender();
        let old_a_hook = queued_hot_reload_hook(sender_a.clone());
        let registration_a = register_request_rebuild(queued_hot_reload_hook(sender_a));
        let _realm_a = install_platform_realm(runtime_a, &test_window());
        teardown_platform_realm();

        let runtime_b = super::super::ui_realm::UiRealm::for_test();
        let sender_b = runtime_b.command_sender();
        let realm_b = install_platform_realm(runtime_b, &test_window());
        let registration_b = register_request_rebuild(queued_hot_reload_hook(sender_b));
        drop(registration_a);

        old_a_hook();
        let after_old = Rc::new(RefCell::new(None));
        let after_old_in_frame = Rc::clone(&after_old);
        dispatch_platform_realm(
            realm_b,
            RealmTask::Frame(Box::new(move |realm| {
                *after_old_in_frame.borrow_mut() = Some(realm.drain_commands());
            })),
        )
        .expect("B frame dispatches");
        assert_eq!(
            *after_old.borrow(),
            Some(super::super::ui_realm::DrainReport::default()),
            "stale A hook must not enqueue into B"
        );

        std::thread::spawn(request_rebuild)
            .join()
            .expect("worker-side rebuild request");
        let after_current = Rc::new(RefCell::new(None));
        let after_current_in_frame = Rc::clone(&after_current);
        dispatch_platform_realm(
            realm_b,
            RealmTask::Frame(Box::new(move |realm| {
                *after_current_in_frame.borrow_mut() = Some(realm.drain_commands());
            })),
        )
        .expect("B frame dispatches");
        assert_eq!(
            after_current.borrow().as_ref().map(|report| report.invoked),
            Some(1),
            "current B hook must dispatch exactly once"
        );

        drop(registration_b);
        teardown_platform_realm();
    }

    #[test]
    fn whole_frame_event_keeps_realm_global_key_scope_active() {
        let realm = super::super::ui_realm::UiRealm::for_test();
        let key = flui_view::GlobalKey::<()>::new();
        let element = flui_foundation::ElementId::new(91);
        realm
            .widgets()
            .with_build_owner_mut(|owner| owner.register_global_key(key.id(), element));
        let dispatcher = install_platform_realm(realm, &test_window());
        let key_after_frame = key.clone();

        assert_eq!(key.current_element(), None, "scope starts inactive");
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_| {
                assert_eq!(key.current_element(), Some(element));
            })),
        )
        .expect("frame dispatches");
        assert_eq!(
            key_after_frame.current_element(),
            None,
            "frame scope is restored"
        );
        teardown_platform_realm();
    }

    /// A disabled->enabled lifecycle edge must redirty the root when
    /// delivered the way production actually delivers one: as a
    /// `PlatformToUi::Lifecycle` event through `dispatch_platform_realm`,
    /// which takes the realm OUT of `APP_RUNTIME` for the duration of the
    /// dispatch and only restores it after `emit_lifecycle_transition`
    /// returns. A fire-time `APP_RUNTIME` lookup (an `UpdateScheduler` lifecycle
    /// listener, the previous shape of this fix) can never see the realm
    /// during that exact window — driving a throwaway `UpdateScheduler` directly,
    /// the previous version of this test's approach, never exercises that
    /// window at all, which is why it never caught the bug.
    #[test]
    fn frames_reenable_redirties_root_when_dispatched_through_the_realm_queue() {
        use std::cell::Cell;

        #[derive(Clone)]
        struct LeafView;

        impl flui_view::RenderView for LeafView {
            type Protocol = flui_rendering::protocol::BoxProtocol;
            type RenderObject = flui_objects::RenderSizedBox;

            fn create_render_object(
                &self,
                _ctx: &flui_view::RenderObjectContext<'_>,
            ) -> Self::RenderObject {
                flui_objects::RenderSizedBox::shrink()
            }

            fn update_render_object(
                &self,
                _ctx: &flui_view::RenderObjectContext<'_>,
                render_object: &mut Self::RenderObject,
            ) {
                *render_object = flui_objects::RenderSizedBox::shrink();
            }
        }

        impl View for LeafView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::render_variable(self)
            }
        }

        struct CountingRasterBackend {
            render_scene_calls: u32,
        }

        impl CountingRasterBackend {
            fn new() -> Self {
                Self {
                    render_scene_calls: 0,
                }
            }
        }

        impl flui_engine::RasterBackend for CountingRasterBackend {
            fn render_scene(
                &mut self,
                _scene: &flui_layer::Scene,
            ) -> Result<bool, flui_engine::EngineError> {
                self.render_scene_calls += 1;
                Ok(true)
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
            }
            fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
            fn mark_full_repaint(&mut self) {}
            fn has_damage(&self) -> bool {
                true
            }
            fn size(&self) -> (u32, u32) {
                (800, 600)
            }
            fn reconfigure_surface(&mut self) -> Result<(), flui_engine::EngineError> {
                Ok(())
            }
        }

        let dispatcher = install_test_realm();
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                realm
                    .attach_root_widget(&LeafView)
                    .expect("attach succeeds");
            })),
        )
        .expect("attach dispatches");

        // Consume the post-attach dirty flag with one real frame first, so
        // the pipeline is genuinely idle going into the lifecycle dance
        // below -- otherwise the later paint this test asserts on could be
        // explained by left-over dirt from attach, not by the redirty under
        // test.
        let initial_presented = Rc::new(Cell::new(false));
        let initial_presented_in_frame = Rc::clone(&initial_presented);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |realm| {
                let mut backend = CountingRasterBackend::new();
                initial_presented_in_frame.set(realm.render_frame_entered(&mut backend));
            })),
        )
        .expect("initial frame dispatches");
        assert!(
            initial_presented.get(),
            "precondition: the attached root must present on its first frame"
        );

        let root_is_clean = Rc::new(Cell::new(false));
        let root_is_clean_in_frame = Rc::clone(&root_is_clean);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |realm| {
                root_is_clean_in_frame.set(!realm.needs_redraw());
            })),
        )
        .expect("clean-check dispatches");
        assert!(
            root_is_clean.get(),
            "precondition: the root must be clean (Idle) going into the lifecycle dance"
        );

        // The disable edge, delivered the way production actually delivers
        // a lifecycle transition: as a `PlatformToUi::Lifecycle` event
        // through the realm dispatch queue, which takes the realm OUT of
        // `APP_RUNTIME` for the duration of the dispatch.
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Hidden)),
        )
        .expect("hidden dispatches");

        // The re-enable edge under test, same delivery path.
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        )
        .expect("resumed dispatches");

        let repainted = Rc::new(Cell::new(false));
        let repainted_in_frame = Rc::clone(&repainted);
        let render_scene_calls = Rc::new(Cell::new(0u32));
        let render_scene_calls_in_frame = Rc::clone(&render_scene_calls);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |realm| {
                let mut backend = CountingRasterBackend::new();
                repainted_in_frame.set(realm.render_frame_entered(&mut backend));
                render_scene_calls_in_frame.set(backend.render_scene_calls);
            })),
        )
        .expect("post-reenable frame dispatches");

        assert!(
            repainted.get(),
            "a disabled->enabled lifecycle edge delivered through the real realm dispatch \
             queue must redirty the root so the next frame actually presents, not stay Idle \
             -- this is the exact stale-window-on-resume bug the redirty logic exists to \
             prevent"
        );
        assert_eq!(
            render_scene_calls.get(),
            1,
            "the redirty must produce real paint output, not merely flip a flag \
             render_frame_entered ignores"
        );

        teardown_platform_realm();
    }

    // ========================================================================
    // Issue #555 red exploits: multi-realm `AppRuntime` hosting,
    // separate-realms policy, exit policy, hot-restart.
    //
    // Every test below was structurally impossible to even set up before this
    // change: `AppRuntime` had exactly one `Option<UiRealm>` slot, so a
    // second `install_platform_realm`/`install_realm_alongside` call always
    // displaced the first rather than coexisting with it. There was no red
    // run to capture (the code to call did not exist), so "red" here means
    // "would not compile / had no second-realm install path to call" rather
    // than "compiled and failed an assertion" -- stated rather than
    // fabricated.
    // ========================================================================

    /// Installs two coexisting realms, A (through the legacy displacing
    /// `install_platform_realm`) and B (through `install_realm_alongside`,
    /// non-displacing). Both windows come from ONE shared headless platform
    /// instance, not two separate `test_window()` calls: `HeadlessPlatform`
    /// mints window ids from an instance-local counter, so two windows from
    /// two SEPARATE `headless_platform()` calls can alias the same id —
    /// fatal for two realms meant to coexist, since `WindowRegistry::
    /// register_window` REPLACES on a matching id, silently dropping realm
    /// A's window mapping the moment realm B's aliased-id window installs.
    fn install_two_test_realms() -> (RealmDispatcher, RealmDispatcher) {
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");
        let dispatcher_a =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);
        let dispatcher_b =
            install_realm_alongside(super::super::ui_realm::UiRealm::for_test(), &window_b)
                .expect("realm B installs alongside realm A cleanly");
        (dispatcher_a, dispatcher_b)
    }

    /// Two realms hosted by the SAME `AppRuntime` share no gesture-arena
    /// state: a pointer dispatched only to realm A must leave realm B's
    /// arena completely untouched. Both realms are reached only through
    /// `install_platform_realm`/`install_realm_alongside` and
    /// `dispatch_platform_realm` here -- never a directly-held `UiRealm`
    /// handle -- so this is the "through `AppRuntime`" half of the
    /// end-state invariant; `ui_realm.rs`'s `two_realms_coexist_same_thread`
    /// family already proves the same disjointness at the `UiRealm`
    /// construction level.
    #[test]
    fn two_window_realms_share_no_ui_state_through_app_runtime() {
        let (dispatcher_a, dispatcher_b) = install_two_test_realms();

        assert_ne!(
            dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
            "AppRuntime must never host two live realms under the same identity"
        );

        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(|realm| {
                let down = down_input(4.0);
                if let PlatformInput::Pointer(event) = down {
                    realm
                        .gestures()
                        .handle_pointer_event(&event, |_| HitTestResult::new());
                }
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    1,
                    "realm A observes its own pointer"
                );
            })),
        )
        .expect("A dispatches");

        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    0,
                    "realm B's gesture arena must be untouched by a pointer dispatched only \
                     to realm A -- AppRuntime shares no mutable UI state between hosted realms"
                );
            })),
        )
        .expect("B dispatches independently of A");

        teardown_platform_realm();
    }

    /// `AppRuntime::next_wake` (the wall-clock wake seam) computes the
    /// MIN wake deadline across every hosted realm on this loop thread — an
    /// idle realm with an earlier-armed deadline must win over a busier
    /// sibling with a later one, regardless of install order. Realm A gets
    /// a far (5s) long-press timeout; realm B (installed second) gets a
    /// near (50ms) one — the aggregate must reflect B's, not A's, and not
    /// merely "whichever realm happens to be dispatched last".
    ///
    /// The recognizers are kept alive in `recognizers` for the whole test:
    /// `GestureArena`'s deadline registry holds only a `Weak` reference, so
    /// a recognizer dropped at the end of its own dispatch closure would
    /// silently vanish from `next_deadline`'s snapshot before this test ever
    /// reads it.
    #[test]
    fn next_wake_is_the_min_deadline_across_every_installed_realm() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use flui_interaction::{
            GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
        };

        let (dispatcher_a, dispatcher_b) = install_two_test_realms();
        // `Rc`, not `Arc`: `LongPressGestureRecognizer` is owner-thread-affine
        // (`!Send`/`!Sync`, like every gesture recognizer), and this vec
        // never needs to cross a thread -- both dispatches below run on this
        // same test thread.
        let recognizers: Rc<RefCell<Vec<Arc<LongPressGestureRecognizer>>>> =
            Rc::new(RefCell::new(Vec::new()));

        let recognizers_a = Rc::clone(&recognizers);
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                let arena = realm.gestures().arena().clone();
                let recognizer = LongPressGestureRecognizer::with_settings(
                    arena,
                    GestureSettings::touch_defaults()
                        .with_long_press_timeout(std::time::Duration::from_secs(5)),
                )
                .with_on_long_press(|| {});
                let pointer = PointerId::new(2).expect("nonzero pointer id");
                recognizer.add_pointer(
                    pointer,
                    flui_types::Offset::new(
                        flui_types::geometry::px(10.0),
                        flui_types::geometry::px(10.0),
                    ),
                );
                recognizers_a.borrow_mut().push(recognizer);
            })),
        )
        .expect("A dispatches");

        let before_b = std::time::Instant::now();
        let recognizers_b = Rc::clone(&recognizers);
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(move |realm| {
                let arena = realm.gestures().arena().clone();
                let recognizer = LongPressGestureRecognizer::with_settings(
                    arena,
                    GestureSettings::touch_defaults()
                        .with_long_press_timeout(std::time::Duration::from_millis(50)),
                )
                .with_on_long_press(|| {});
                let pointer = PointerId::new(3).expect("nonzero pointer id");
                recognizer.add_pointer(
                    pointer,
                    flui_types::Offset::new(
                        flui_types::geometry::px(20.0),
                        flui_types::geometry::px(20.0),
                    ),
                );
                recognizers_b.borrow_mut().push(recognizer);
            })),
        )
        .expect("B dispatches");

        let next_wake = APP_RUNTIME
            .with(|slot| slot.borrow().next_wake())
            .expect("two armed deadlines pending -- next_wake must be Some");

        let until_wake = next_wake.saturating_duration_since(before_b);
        assert!(
            until_wake < std::time::Duration::from_secs(1),
            "next_wake must reflect realm B's near (50ms) deadline, not realm A's far (5s) \
             one -- got {until_wake:?} until wake"
        );

        drop(recognizers);
        teardown_platform_realm();
    }

    /// Regression pin, driving the real production sequence: `AppRuntime::
    /// next_wake` must never surface a deadline that nothing on the wake
    /// path drains. `GestureTimerService` (`flui_interaction::
    /// global_timer_service`) has no production caller of
    /// `check_timers()` reachable from `UiRealm::draw_frame_entered`/
    /// `tick_deadlines` — an earlier version of `next_wake` folded its
    /// `next_deadline()` into the aggregate anyway, so a timer scheduled
    /// through the public `global_timer_service()` API could arm a
    /// `WaitUntil` that, once past, never gets cleared: the very next
    /// `about_to_wait` recomputes the identical past instant, the same
    /// unbounded-spin shape the missing-actuator finding closed from the
    /// other direction. A standalone winit probe (kept outside this
    /// repo's automated suite) measured ~310K–390K `about_to_wait`
    /// iterations per 500ms with the fold-in and an actuator present, vs.
    /// 6 with the source excluded (this method's current shape).
    ///
    /// This test installs a realm with zero armed deadlines of its own,
    /// schedules an already-due timer directly on the ambient
    /// `global_timer_service()`, and asserts `next_wake()` answers `None`
    /// — the due, undrained timer must never surface into the wall-clock
    /// wake computation at all.
    #[test]
    fn next_wake_never_surfaces_the_global_timer_services_undrained_deadline() {
        let dispatcher = install_test_realm();
        dispatch_platform_realm(dispatcher, RealmTask::Frame(Box::new(|_realm| {})))
            .expect("realm dispatches with nothing armed");

        let fired = Arc::new(AtomicBool::new(false));
        let fired_in_timer = Arc::clone(&fired);
        let timer = flui_interaction::global_timer_service().schedule(
            std::time::Duration::ZERO,
            move || {
                fired_in_timer.store(true, std::sync::atomic::Ordering::SeqCst);
            },
        );

        let next_wake = APP_RUNTIME.with(|slot| slot.borrow().next_wake());
        assert!(
            next_wake.is_none(),
            "a due timer on the ambient global_timer_service() must not surface into \
             next_wake() -- nothing on the wake path drains it, so folding it in would \
             re-arm the same past instant on every about_to_wait forever"
        );

        // Drain it explicitly so this test doesn't leak a live, still-armed
        // timer into whatever runs next in this process — `global_timer_
        // service()` is process-global BY DESIGN (see `ui_realm.rs`'s
        // `dropping_realm_a_cannot_wake_realm_b`), so under `cargo test`'s
        // shared-process parallelism (never under `cargo nextest run`,
        // which gives this test its own process) it may already have been
        // drained by a concurrently-running test; either way, this call is
        // idempotent and the callback firing (or not, if raced away) is not
        // itself under test here.
        flui_interaction::global_timer_service().check_timers();
        let _ = fired.load(std::sync::atomic::Ordering::SeqCst);
        drop(timer);

        teardown_platform_realm();
    }

    /// A rejected nested cross-realm dispatch must never smuggle its task
    /// into the target realm's queue: the task must not run during the
    /// rejected attempt, AND must not be silently delivered by the NEXT
    /// legitimate dispatch to that same realm either. Before the ordering
    /// fix, the event was pushed into realm B's queue BEFORE the
    /// nested-dispatch guard ran, so nothing ever removed a rejected
    /// event — the very next legitimate dispatch to B would pop and run it.
    #[test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the nested-dispatch guard fires via debug_assert!(false, ...) in this build; \
                  release builds return Err without panicking, so realm A's own outer dispatch \
                  never unwinds and this test's catch_unwind expectation does not apply"
    )]
    fn rejected_nested_dispatch_never_delivers_its_task_on_the_next_legitimate_dispatch() {
        let (dispatcher_a, dispatcher_b) = install_two_test_realms();

        let rejected_ran = Rc::new(Cell::new(false));
        let rejected_ran_in_task = Rc::clone(&rejected_ran);

        // From inside realm A's own dispatched task, attempt a NESTED
        // dispatch to realm B -- a DIFFERENT, resident-and-idle realm --
        // which the nested-cross-realm-dispatch guard must reject before
        // ever touching B's queue. The guard's debug_assert! panics in this
        // build; the panic unwinds through A's own dispatch, which this
        // catch_unwind survives.
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_platform_realm(
                dispatcher_a,
                RealmTask::Frame(Box::new(move |_realm| {
                    let _ = dispatch_platform_realm(
                        dispatcher_b,
                        RealmTask::Frame(Box::new(move |_| {
                            rejected_ran_in_task.set(true);
                        })),
                    );
                })),
            )
        }));
        assert!(
            attempt.is_err(),
            "the nested cross-realm dispatch attempt must panic via the guard's debug_assert!"
        );

        // A LEGITIMATE, top-level dispatch to realm B, driven afterward.
        dispatch_platform_realm(dispatcher_b, RealmTask::Frame(Box::new(|_| {})))
            .expect("realm B still dispatches normally after the rejected nested attempt");

        assert!(
            !rejected_ran.get(),
            "the REJECTED task must never run -- not during the rejected attempt, and not \
             smuggled into realm B's queue for a later legitimate dispatch to deliver"
        );

        teardown_platform_realm();
    }

    /// Realm A tearing itself down from WITHIN its own dispatched task must
    /// not disturb sibling realm B's ability to keep dispatching and
    /// producing (presenting) frames.
    #[test]
    fn teardown_realm_a_mid_dispatch_leaves_realm_b_frame_producing() {
        use flui_widgets::SizedBox;

        struct AlwaysPresentsBackend;
        impl flui_engine::RasterBackend for AlwaysPresentsBackend {
            fn render_scene(
                &mut self,
                _scene: &flui_layer::Scene,
            ) -> Result<bool, flui_engine::EngineError> {
                Ok(true)
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
            }
            fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
            fn mark_full_repaint(&mut self) {}
            fn has_damage(&self) -> bool {
                true
            }
            fn size(&self) -> (u32, u32) {
                (64, 64)
            }
            fn reconfigure_surface(&mut self) -> Result<(), flui_engine::EngineError> {
                Ok(())
            }
        }

        let (dispatcher_a, dispatcher_b) = install_two_test_realms();
        let realm_a_id = dispatcher_a.address.realm_id;

        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(|realm| {
                realm
                    .attach_root_widget_with_size(&SizedBox::new(20.0, 20.0), 20.0, 20.0)
                    .expect("B mounts its own root");
            })),
        )
        .expect("B's attach dispatches");

        // Realm A tears itself down from WITHIN its own dispatched task. The
        // defer-to-idle discipline (`AppRuntime::request_realm_uninstall`)
        // means this queues rather than nests -- applied once this task's
        // own dispatch restores below.
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |_realm| {
                uninstall_platform_realm(realm_a_id);
            })),
        )
        .expect("A's own mid-dispatch teardown task runs");

        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_none()),
            "realm A must be gone once its own mid-dispatch uninstall request has applied"
        );

        let presented = Rc::new(RefCell::new(false));
        let presented_in_frame = Rc::clone(&presented);
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(move |realm| {
                let mut backend = AlwaysPresentsBackend;
                *presented_in_frame.borrow_mut() = realm.render_frame_entered(&mut backend);
            })),
        )
        .expect("realm B still dispatches after realm A's mid-dispatch teardown");
        assert!(
            *presented.borrow(),
            "realm B must still produce (and present) a frame after a sibling realm tore \
             itself down mid-dispatch"
        );

        teardown_platform_realm();
    }

    /// `install_realm_alongside` must REFUSE a window-id collision, not
    /// silently re-route the colliding window's mapping onto the new realm
    /// (`WindowRegistry::register_window`'s replace semantics would do
    /// exactly that -- the wrong tool for two realms meant to coexist, and
    /// the exact failure mode this crate's own `HeadlessPlatform`
    /// window-id-aliasing gotcha produced during development).
    #[test]
    fn install_realm_alongside_refuses_a_colliding_window_id_instead_of_silently_rerouting() {
        let platform = flui_platform::headless_platform();
        let window = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window");
        let dispatcher_a =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);

        let result = install_realm_alongside(super::super::ui_realm::UiRealm::for_test(), &window);
        match result {
            Err(super::super::window_registry::RegistryError::WindowAlreadyMapped { existing }) => {
                assert_eq!(
                    existing, dispatcher_a.address,
                    "the refusal must report realm A's own address as the existing mapping"
                );
            }
            other => panic!(
                "a colliding window id must be refused as WindowAlreadyMapped, got {other:?}"
            ),
        }

        assert!(
            APP_RUNTIME.with(|slot| slot
                .borrow()
                .realms
                .get(&dispatcher_a.address.realm_id)
                .is_some()),
            "realm A's own registration must be untouched by the refused collision"
        );

        teardown_platform_realm();
    }

    /// The DEFERRED half of the collision-refusal path:
    /// `drain_pending_realm_mutations`'s own `Install` arm, reached only
    /// when a collision is requested mid-visit/mid-dispatch rather than at
    /// idle. Must behave identically to the immediate path above -- refuse,
    /// leave realm A's own registration untouched, and never corrupt the
    /// registry -- and must do so without ever dropping the rejected
    /// `RealmSlot` (and the `UiRealm` it owns) while this visit's own
    /// `APP_RUNTIME` borrow is live: `apply_install`/
    /// `drain_pending_realm_mutations` route a rejected slot through
    /// `removed` instead, the same bucket a removed `Uninstall` slot uses,
    /// dropped only once the caller's borrow has released.
    #[test]
    fn deferred_install_collision_is_refused_without_corrupting_the_registry() {
        let platform = flui_platform::headless_platform();
        let window = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window");
        let dispatcher_a =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);
        let realm_a_id = dispatcher_a.address.realm_id;

        let colliding_realm = super::super::ui_realm::UiRealm::for_test();
        let colliding_address = flui_foundation::PresentationAddress {
            realm_id: colliding_realm.realm_id(),
            presentation_id: colliding_realm.presentation_id(),
        };
        let mut colliding_realm = Some(colliding_realm);

        for_each_installed_realm(|realm| {
            assert_eq!(
                realm.realm_id(),
                realm_a_id,
                "the sole installed realm is realm A"
            );
            let deferred = APP_RUNTIME.with(|slot| {
                slot.borrow_mut().request_realm_install(
                    colliding_address.realm_id,
                    RealmSlot {
                        realm: colliding_realm.take(),
                        queue: VecDeque::new(),
                        draining: false,
                        address: colliding_address,
                        surface_applier: None,
                    },
                    std::sync::Arc::clone(&window),
                )
            });
            assert!(
                deferred.is_ok(),
                "an Install requested mid-visit must defer (Ok), not fail synchronously -- \
                 the collision is only discovered once the deferred drain applies it"
            );
        });

        assert!(
            APP_RUNTIME.with(|slot| slot
                .borrow()
                .realms
                .get(&colliding_address.realm_id)
                .is_none()),
            "the colliding realm must never enter the registry -- its window id collided with \
             realm A's own mapping when the deferred drain applied it"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_some()),
            "realm A's own registration must survive the sibling's refused, deferred collision"
        );

        teardown_platform_realm();
    }

    /// Closing one of two hosted windows must not exit the loop while a
    /// sibling realm is still hosted (the default `ExitPolicy::OnLastWindowClosed`).
    #[test]
    fn closing_one_of_two_windows_does_not_exit_the_loop() {
        let (dispatcher_a, dispatcher_b) = install_two_test_realms();

        uninstall_platform_realm(dispatcher_a.address.realm_id);

        let should_exit = APP_RUNTIME.with(|slot| {
            let (exit, removed) = slot
                .borrow_mut()
                .should_exit(ExitPolicy::OnLastWindowClosed);
            drop(removed);
            exit
        });
        assert!(
            !should_exit,
            "closing one of two windows must not exit the loop while a sibling realm is \
             still hosted"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot
                .borrow()
                .realms
                .get(&dispatcher_b.address.realm_id)
                .is_some()),
            "the surviving realm B must still be hosted"
        );

        teardown_platform_realm();
    }

    /// Installs realm A via the legacy single-window path (mirroring
    /// `install_test_realm`), through a REAL `OwnerPlatform` (unlike
    /// `install_test_realm`/`install_two_test_realms`, which never install
    /// one) — both windows this helper and its caller open come from the
    /// SAME headless platform instance, so their native window identities
    /// cannot collide once both are registered in the one `WindowRegistry`.
    fn install_realm_a_through_a_real_owner_platform() -> (RealmDispatcher, OwnerHostClearGuard) {
        use std::{cell::Cell, rc::Rc};

        let clear_guard = OwnerHostClearGuard::arm();
        let platform = flui_platform::headless_platform();
        let dispatcher_a_slot: Rc<Cell<Option<RealmDispatcher>>> = Rc::new(Cell::new(None));
        let dispatcher_a_slot_for_on_ready = Rc::clone(&dispatcher_a_slot);
        let ready = platform.run(Box::new(move |owner| {
            install_owner_platform(owner);
            let window_a = with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("BUG: install_owner_platform just ran above")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless open_window is always Ready");
            dispatcher_a_slot_for_on_ready.set(Some(install_platform_realm(
                super::super::ui_realm::UiRealm::for_test(),
                &window_a,
            )));
            Ok(())
        }));
        ready.expect("installing realm A must not fail");
        (
            dispatcher_a_slot.get().expect("set inside on_ready above"),
            clear_guard,
        )
    }

    /// `WindowPolicy::SeparateRealms`, driven through the REAL embedder seam
    /// (`open_secondary_window`) rather than a direct
    /// `install_realm_alongside` call — proves the POLICY-driven fork itself
    /// routes to the share-nothing path, not just the underlying primitive
    /// (`two_window_realms_share_no_ui_state_through_app_runtime` already
    /// covers that). Same oracle: a pointer dispatched only to realm A must
    /// leave realm B's gesture arena completely untouched.
    #[test]
    fn two_realms_via_separate_windows_policy_share_nothing() {
        let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

        open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms)
            .expect("WindowPolicy::SeparateRealms must install a second realm cleanly");

        let dispatcher_b = APP_RUNTIME
            .with(|slot| {
                let state = slot.borrow();
                let (_, slot_b) = state
                    .realms
                    .iter()
                    .find(|(id, _)| *id != dispatcher_a.address.realm_id)?;
                Some(RealmDispatcher {
                    owner_thread: state.owner_thread?,
                    address: slot_b.address,
                })
            })
            .expect("open_secondary_window(SeparateRealms) must install a second, distinct realm");

        assert_ne!(
            dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
            "WindowPolicy::SeparateRealms must install a genuinely distinct realm, never \
             displace or merge into realm A"
        );

        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(|realm| {
                let down = down_input(4.0);
                if let PlatformInput::Pointer(event) = down {
                    realm
                        .gestures()
                        .handle_pointer_event(&event, |_| HitTestResult::new());
                }
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    1,
                    "realm A observes its own pointer"
                );
            })),
        )
        .expect("A dispatches");

        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    0,
                    "the policy-driven secondary realm must share no gesture-arena state with \
                     realm A -- open_secondary_window(SeparateRealms) must route through \
                     install_realm_alongside, never a path that merges state with a sibling"
                );
            })),
        )
        .expect("B dispatches independently of A");

        teardown_platform_realm();
    }

    /// `WindowPolicy::SharedRealm`, driven through the REAL embedder seam
    /// (`open_secondary_window`) — proves this really is forest-membership
    /// routing (a second PRESENTATION of the SAME realm), not a second realm
    /// in disguise: the hosted-realm count must stay at one, while the
    /// realm's own presentation count grows from one to two.
    #[test]
    fn one_realm_two_windows_policy_routes_by_presentation() {
        let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

        let (realm_count_before, presentation_count_before) = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            let realm_count = state.realms.iter().count();
            let presentation_count = state
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|slot| slot.realm.as_ref())
                .expect("realm A is resident")
                .presentation_count();
            (realm_count, presentation_count)
        });
        assert_eq!(realm_count_before, 1);
        assert_eq!(presentation_count_before, 1);

        open_secondary_window(AppConfig::default(), WindowPolicy::SharedRealm).expect(
            "WindowPolicy::SharedRealm must install a second presentation into realm A cleanly",
        );

        let (realm_count_after, presentation_count_after) = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            let realm_count = state.realms.iter().count();
            let presentation_count = state
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|slot| slot.realm.as_ref())
                .expect("realm A is still resident -- SharedRealm must not have replaced it")
                .presentation_count();
            (realm_count, presentation_count)
        });
        assert_eq!(
            realm_count_after, realm_count_before,
            "WindowPolicy::SharedRealm must NOT install a second realm -- it routes into the \
             existing one via a second presentation"
        );
        assert_eq!(
            presentation_count_after, 2,
            "WindowPolicy::SharedRealm must install a genuine second presentation into realm \
             A's forest -- real forest-membership routing, not a second realm in disguise"
        );

        teardown_platform_realm();
    }

    /// The live-loop counterpart of `closing_one_of_two_windows_does_not_
    /// exit_the_loop` (which drives `AppRuntime::should_exit` directly).
    /// This one goes through the REAL platform seam end to end:
    /// `install_exit_policy_hook` installs the hook exactly like
    /// `run_desktop`'s bootstrap does, and `window.close()` on each REAL
    /// (headless) native window drives flui-platform's own
    /// `notify_closed`/`CloseRequested`-equivalent bookkeeping — THAT is
    /// what consults the hook, never a direct call into `AppRuntime`.
    /// `uninstall_platform_realm` before each `close()` mirrors
    /// `run_desktop`'s own `on_close` ordering exactly (see that callback's
    /// doc for why the ordering is load-bearing): the registry must already
    /// reflect the closing window's realm being gone by the time the hook
    /// reads it. Closing one of two windows must not fire the platform's
    /// own `quit` callback; closing the last one must fire it exactly once.
    #[test]
    fn closing_one_of_two_windows_does_not_exit_through_the_real_platform_hook_closing_both_does() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = flui_platform::headless_platform();
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let quit_calls_for_on_ready = Arc::clone(&quit_calls);
        let ready = platform.run(Box::new(move |owner| {
            install_owner_platform(owner);
            install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

            let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
            with_owner_platform(|owner| {
                owner.shared().on_quit(Box::new(move || {
                    quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
                }));
            });

            let window_a = with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("owner installed above")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless open_window is always Ready");
            let window_b = with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("owner installed above")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless open_window is always Ready");

            let dispatcher_a =
                install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);
            let dispatcher_b =
                install_realm_alongside(super::super::ui_realm::UiRealm::for_test(), &window_b)
                    .expect("realm B installs alongside realm A cleanly");

            // Close A: realm B is still resident, so the hook must veto.
            uninstall_platform_realm(dispatcher_a.address.realm_id);
            window_a.close();
            assert_eq!(
                quit_calls_for_on_ready.load(Ordering::SeqCst),
                0,
                "closing one of two windows must not fire the platform's own quit callback"
            );

            // Close B (the last one): the registry is now empty, so the
            // hook must allow the exit and the platform's own quit must
            // fire exactly once.
            uninstall_platform_realm(dispatcher_b.address.realm_id);
            window_b.close();
            assert_eq!(
                quit_calls_for_on_ready.load(Ordering::SeqCst),
                1,
                "closing the LAST window must fire the platform's own quit callback exactly once"
            );

            Ok(())
        }));
        ready.expect("on_ready must not fail");

        teardown_platform_realm();
    }

    /// Hot-restart's own visit primitive (`for_each_installed_realm`) must
    /// see exactly what each `WindowPolicy` claims to have installed --
    /// proven for both modes, each opened through the REAL
    /// `open_secondary_window` seam (not a direct
    /// `install_realm_alongside`/`install_presentation_alongside` call).
    #[test]
    fn hot_restart_with_two_realms_and_with_two_presentations() {
        // Mode 1: WindowPolicy::SeparateRealms -- hot-restart must visit TWO
        // distinct realms.
        {
            let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();
            open_secondary_window(AppConfig::default(), WindowPolicy::SeparateRealms)
                .expect("WindowPolicy::SeparateRealms must install a second realm cleanly");

            let mut visited_realms = Vec::new();
            for_each_installed_realm(|realm| {
                visited_realms.push(realm.realm_id());
            });

            assert_eq!(
                visited_realms.len(),
                2,
                "hot-restart must visit both policy-installed realms"
            );
            assert!(
                visited_realms.contains(&dispatcher_a.address.realm_id),
                "hot-restart's visit must include realm A"
            );

            teardown_platform_realm();
        }

        // Mode 2: WindowPolicy::SharedRealm -- hot-restart must visit
        // exactly ONE realm, carrying BOTH presentations.
        {
            let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();
            open_secondary_window(AppConfig::default(), WindowPolicy::SharedRealm).expect(
                "WindowPolicy::SharedRealm must install a second presentation into realm A \
                 cleanly",
            );

            let mut visited: Vec<(RealmId, usize)> = Vec::new();
            for_each_installed_realm(|realm| {
                visited.push((realm.realm_id(), realm.presentation_count()));
            });

            assert_eq!(
                visited,
                vec![(dispatcher_a.address.realm_id, 2)],
                "hot-restart must visit exactly the one SharedRealm-hosted realm, carrying both \
                 presentations through the visit -- never split into two realms"
            );

            teardown_platform_realm();
        }
    }

    /// Like [`install_realm_a_through_a_real_owner_platform`], but ALSO
    /// installs the exit-policy hook (mirroring `run_desktop`'s own
    /// bootstrap exactly) and an `on_quit` counter — for tests that need to
    /// drive a real window close through the REAL platform hook end to end,
    /// not a direct `AppRuntime::should_exit` call.
    fn install_realm_a_with_exit_policy_and_quit_counter() -> (
        RealmDispatcher,
        std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
        Arc<std::sync::atomic::AtomicUsize>,
        OwnerHostClearGuard,
    ) {
        use std::{cell::RefCell, rc::Rc, sync::atomic::AtomicUsize};

        type Installed = (
            RealmDispatcher,
            std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
        );

        let clear_guard = OwnerHostClearGuard::arm();
        let platform = flui_platform::headless_platform();
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let quit_calls_for_on_ready = Arc::clone(&quit_calls);
        let installed_slot: Rc<RefCell<Option<Installed>>> = Rc::new(RefCell::new(None));
        let installed_slot_for_on_ready = Rc::clone(&installed_slot);
        let ready = platform.run(Box::new(move |owner| {
            install_owner_platform(owner);
            install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

            let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
            with_owner_platform(|owner| {
                owner.shared().on_quit(Box::new(move || {
                    quit_calls_for_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }));
            });

            let window_a = with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("owner installed above")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless open_window is always Ready");
            let dispatcher_a =
                install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);
            // Mirror `run_desktop`'s own `on_close` wiring exactly, so a
            // real `window_a.close()` in a test drives the SAME production
            // path a real desktop bootstrap would.
            window_a.on_close(Box::new(move || {
                close_this_window(dispatcher_a);
            }));
            *installed_slot_for_on_ready.borrow_mut() = Some((dispatcher_a, window_a));
            Ok(())
        }));
        ready.expect("installing realm A must not fail");
        let (dispatcher_a, window_a) = installed_slot
            .borrow_mut()
            .take()
            .expect("set inside on_ready above");
        (dispatcher_a, window_a, quit_calls, clear_guard)
    }

    /// `open_secondary_window`'s own `on_close` wiring, not a test manually
    /// calling `uninstall_platform_realm`/`close_presentation` itself, must
    /// be what tears the secondary window's realm down. Before
    /// the fix this window's `on_close` only logged — closing it left its
    /// realm resident in `AppRuntime` forever, so closing the app's LAST
    /// window afterward never actually emptied the registry and the
    /// exit-policy hook vetoed the exit forever (a production hang: zero
    /// windows left, app still alive). Drives BOTH closes through their own
    /// registered `on_close`: `close_this_window` for the primary — the
    /// exact primitive `run_desktop`'s bootstrap itself calls, since a full
    /// GPU-backed desktop bootstrap cannot run in a unit test — and a REAL
    /// `window_b.close()` for the secondary, exercising
    /// `open_secondary_window`'s own wiring, never a hand-rolled substitute.
    #[test]
    fn separate_realms_secondary_window_close_tears_down_its_own_realm_and_allows_exit() {
        use std::sync::atomic::Ordering;

        let (dispatcher_a, window_a, quit_calls, _clear_guard) =
            install_realm_a_with_exit_policy_and_quit_counter();

        let (dispatcher_b, window_b) =
            open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                .expect("WindowPolicy::SeparateRealms must install a second realm cleanly")
                .expect("headless open_window is always Ready, never Pending");
        assert_ne!(
            dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
            "SeparateRealms must install a genuinely distinct realm"
        );

        // Close A first, through its OWN real on_close (siblings survive)
        // -- must not exit.
        window_a.close();
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "closing one of two windows must not exit while the sibling realm survives"
        );

        // Close B through its OWN registered `on_close` -- a REAL
        // `window.close()`, never a manual `uninstall_platform_realm`/
        // `close_presentation` call from this test. This is the exact call
        // that was log-only before the fix.
        window_b.close();
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "closing the secondary window through its own real on_close must have uninstalled \
             its realm -- if open_secondary_window's on_close is ever log-only again, this \
             realm stays resident forever"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            1,
            "closing the LAST window (the secondary, through its own real on_close) must allow \
             the exit exactly once"
        );

        teardown_platform_realm();
    }

    /// The `WindowPolicy::SharedRealm` counterpart: closing the secondary
    /// presentation through its OWN real `on_close` must remove only ITSELF
    /// — the shared realm and the still-open primary must survive
    /// untouched — and must NOT exit while the primary survives. Before the
    /// fix, `open_secondary_window`'s on_close was log-only for this policy
    /// too, so the secondary presentation would have stayed forest-resident
    /// (and, worse, focus/keyboard-addressable) forever after its native
    /// window was gone.
    #[test]
    fn shared_realm_secondary_presentation_close_removes_only_itself_and_keeps_the_realm_alive() {
        use std::sync::atomic::Ordering;

        let (dispatcher_a, window_a, quit_calls, _clear_guard) =
            install_realm_a_with_exit_policy_and_quit_counter();

        let (dispatcher_b, window_b) = open_secondary_window_impl(
            AppConfig::default(),
            WindowPolicy::SharedRealm,
        )
        .expect("WindowPolicy::SharedRealm must install a second presentation into realm A cleanly")
        .expect("headless open_window is always Ready, never Pending");
        assert_eq!(
            dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
            "SharedRealm must route into the SAME realm as the primary"
        );

        // Close B (a live non-sole presentation) through its own real
        // on_close -- must remove only itself, never the shared realm.
        window_b.close();
        let (realm_count, presentation_count) = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            let realm_count = state.realms.iter().count();
            let presentation_count = state
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|slot| slot.realm.as_ref())
                .expect(
                    "the shared realm must still be resident -- B's own close must not \
                         have torn down the whole realm out from under the still-open primary",
                )
                .presentation_count();
            (realm_count, presentation_count)
        });
        assert_eq!(
            realm_count, 1,
            "the shared realm must survive B's own close"
        );
        assert_eq!(
            presentation_count, 1,
            "only B's own presentation must be removed; the primary's survives"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "the realm (and its primary presentation) still lives -- this must not exit"
        );

        // Now close the primary too, through its own real on_close (the
        // realm's last presentation) -- must exit.
        window_a.close();
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "closing the realm's last surviving presentation must uninstall the whole realm"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            1,
            "closing the last presentation of the last realm must allow the exit exactly once"
        );

        teardown_platform_realm();
    }

    /// `open_secondary_window`'s own `window.on_input` wiring dispatches
    /// `RealmTask::Event(PlatformToUi::Input(..))` addressed to the
    /// secondary window's own dispatcher — proves BOTH halves of that
    /// routing: the secondary's OWN gesture arena receives it (receipt,
    /// untested by `two_realms_via_separate_windows_policy_share_nothing`,
    /// which only asserts sibling non-receipt), and the primary's arena
    /// stays untouched (the established non-primary lesson —
    /// `input_stamped_for_b_never_reaches_as_arena`'s own review-driven fix
    /// — applied here through the POLICY-DRIVEN seam, not inverted to only
    /// prove non-receipt as the original share-nothing test did).
    #[test]
    fn separate_realms_secondary_window_input_reaches_its_own_realm_not_the_primarys() {
        let (dispatcher_a, _clear_guard) = install_realm_a_through_a_real_owner_platform();

        let (dispatcher_b, _window_b) =
            open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                .expect("WindowPolicy::SeparateRealms must install a second realm cleanly")
                .expect("headless open_window is always Ready, never Pending");

        // Exactly the call `open_secondary_window`'s own `window.on_input`
        // callback makes for window B's native input.
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Event(PlatformToUi::Input(down_input(4.0))),
        )
        .expect("B dispatches");

        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    1,
                    "the secondary window's own realm must receive input dispatched through \
                     its own on_input wiring"
                );
            })),
        )
        .expect("B dispatches");

        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(|realm| {
                assert_eq!(
                    realm.gestures().active_pointer_count(),
                    0,
                    "input addressed to the secondary window's realm must never reach the \
                     primary's -- the established non-primary lesson, not inverted"
                );
            })),
        )
        .expect("A dispatches");

        teardown_platform_realm();
    }

    /// The P1 fix's own end-to-end probe: `open_secondary_window` must not
    /// assume `OwnerPlatform::open_window` always resolves `Ready` — the
    /// real winit owner lane returns `Pending` for any call after
    /// `on_ready`, exactly the calling convention this function documents
    /// as its own intended use (from a callback the FIRST window already
    /// registered). `HeadlessPlatform::enable_deferred_window_open`
    /// exercises that same arm without a real event loop. Drives the
    /// request all the way through: Pending -> a manual `resolve_next` ->
    /// the driver realm's own frame pump completing the install via
    /// `finish_open_secondary_window` -> both windows closing cleanly and
    /// the exit-policy hook firing exactly once.
    #[test]
    fn open_secondary_window_completes_through_the_pending_arm_like_the_real_winit_owner_lane() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Needed to call `.run` on the concrete `HeadlessPlatform` boxed
        // below (a `Box<dyn Platform>`, unlike `headless_platform()`'s own
        // return type, needs the trait in scope for dot-call resolution on
        // a boxed concrete receiver).
        use flui_platform::traits::Platform;

        let clear_guard = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let quit_calls_for_on_ready = Arc::clone(&quit_calls);

        let ready = Box::new(platform).run(Box::new(move |owner| {
            install_owner_platform(owner);
            install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

            let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
            with_owner_platform(|owner| {
                owner.shared().on_quit(Box::new(move || {
                    quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
                }));
            });

            // First window: also deferred, since `enable_deferred_window_open`
            // switches EVERY `open_owner_window` call on this platform, not
            // just the second one -- resolve it the same way real code
            // resolving a `Pending` open would (`try_take` after delivery),
            // proving the mechanism generically before window B ever uses it.
            let mut pending_a = match with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("BUG: install_owner_platform just ran above")
            .expect("deferred mode still accepts the request")
            {
                flui_platform::WindowOpen::Pending(pending) => pending,
                flui_platform::WindowOpen::Ready(_) => {
                    panic!("deferred mode must return Pending, not Ready")
                }
            };
            let resolved_a = deferred
                .resolve_next()
                .expect("window A's request is the only one pending");
            let window_a = pending_a
                .try_take()
                .expect("resolve_next delivers synchronously")
                .expect("mock window creation cannot fail");
            assert!(
                Arc::ptr_eq(&resolved_a, &window_a),
                "resolve_next's own return value must be the SAME window it delivered \
                 through the PendingWindow"
            );

            let dispatcher_a =
                install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);
            window_a.on_close(Box::new(move || {
                close_this_window(dispatcher_a);
            }));

            // Window B: the real subject of this test. `open_secondary_window`
            // must accept the Pending arm and return `None` instead of either
            // erroring or assuming `Ready`.
            let opened =
                open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                    .expect("the Pending arm must be accepted, not treated as an error");
            assert!(
                opened.is_none(),
                "a Pending open must return None -- the install completes asynchronously, \
                 not inline"
            );
            assert_eq!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
                1,
                "nothing must be installed for window B before its Pending open resolves"
            );

            // Resolve the deferred open -- delivers the mock window through
            // the ClaimSlot (waking the spawned task's Waker, which per
            // AsyncDriver's own contract requests a fresh frame) and hands
            // the SAME window handle back here, so this test can drive a
            // REAL platform-level close on it later -- the exact handle
            // `spawn_pending_secondary_window_completion`'s own async block
            // receives and threads into `finish_open_secondary_window`.
            let window_b = deferred
                .resolve_next()
                .expect("exactly one request (window B's) is pending at this point");

            // Drive the driver realm's (realm A's) own frame pump -- the same
            // mechanism a real frame tick uses to poll a woken async task to
            // completion, not a hand-rolled shortcut.
            dispatch_platform_realm(
                dispatcher_a,
                RealmTask::Frame(Box::new(|realm| {
                    realm.scheduler().drive_async_tasks();
                })),
            )
            .expect("realm A dispatches its own frame pump");

            let dispatcher_b = APP_RUNTIME
                .with(|slot| {
                    let state = slot.borrow();
                    let (_, slot_b) = state
                        .realms
                        .iter()
                        .find(|(id, _)| *id != dispatcher_a.address.realm_id)?;
                    Some(RealmDispatcher {
                        owner_thread: state.owner_thread?,
                        address: slot_b.address,
                    })
                })
                .expect(
                    "the driver realm's frame pump must have completed window B's install -- \
                     if the Pending arm is ever silently dropped again, no second realm ever \
                     appears here",
                );
            assert_ne!(
                dispatcher_a.address.realm_id, dispatcher_b.address.realm_id,
                "WindowPolicy::SeparateRealms must install a genuinely distinct realm"
            );

            // Close B through a REAL platform close (`resolve_next`'s own
            // return value, the SAME handle `finish_open_secondary_window`
            // wired) -- exercises `finish_open_secondary_window`'s own
            // `on_close`/`notify_closed` bookkeeping exactly like a real OS
            // close event would, proving the Pending-resolved window is
            // wired identically to the Ready-arm path the two tests above
            // already cover, not merely present in the forest.
            window_b.close();
            assert_eq!(
                quit_calls.load(Ordering::SeqCst),
                0,
                "closing the secondary while the primary survives must not exit"
            );

            window_a.close();
            assert_eq!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
                0,
                "closing the last surviving realm must uninstall it"
            );
            assert_eq!(
                quit_calls.load(Ordering::SeqCst),
                1,
                "closing the LAST window must allow the exit exactly once"
            );

            Ok(())
        }));
        ready.expect("on_ready must not fail");

        teardown_platform_realm();
        drop(clear_guard);
    }

    /// The missing Pending-arm matrix cell: `WindowPolicy::SharedRealm` must
    /// ALSO complete through the Pending arm, not just `SeparateRealms`
    /// above — the harder case. `finish_open_secondary_window`'s
    /// `SharedRealm` branch installs a PRESENTATION into the driver realm
    /// itself, the exact realm `UpdateScheduler::drive_async_tasks` is called
    /// from INSIDE a dispatch of (`dispatched_realm_id` is `Some` for the
    /// whole poll) — and `install_presentation_alongside` has no
    /// defer-to-idle queue of its own; it hard-refuses with
    /// `InstallPresentationError::DispatchInFlight` while a dispatch is in
    /// flight. `spawn_pending_secondary_window_completion`'s own
    /// deferred-completion queue (drained only once the driver realm's
    /// dispatch checkout clears, in `dispatch_platform_realm`'s own tail) is
    /// what makes this arm succeed at all; reverting that deferral
    /// (completing inline from inside the poll again, as an earlier
    /// revision did) reproduces exactly the `DispatchInFlight`
    /// refusal/panic this test exists to rule out.
    #[test]
    fn shared_realm_completes_through_the_pending_arm_like_the_real_winit_owner_lane() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use flui_platform::traits::Platform;

        let clear_guard = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let quit_calls_for_on_ready = Arc::clone(&quit_calls);

        let ready = Box::new(platform).run(Box::new(move |owner| {
            install_owner_platform(owner);
            install_exit_policy_hook(ExitPolicy::OnLastWindowClosed);

            let quit_calls_for_handler = Arc::clone(&quit_calls_for_on_ready);
            with_owner_platform(|owner| {
                owner.shared().on_quit(Box::new(move || {
                    quit_calls_for_handler.fetch_add(1, Ordering::SeqCst);
                }));
            });

            // First window: also deferred (see the SeparateRealms probe
            // above for why) -- resolve it the same way real code resolving
            // a Pending open would.
            let mut pending_a = match with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("BUG: install_owner_platform just ran above")
            .expect("deferred mode still accepts the request")
            {
                flui_platform::WindowOpen::Pending(pending) => pending,
                flui_platform::WindowOpen::Ready(_) => {
                    panic!("deferred mode must return Pending, not Ready")
                }
            };
            let resolved_a = deferred
                .resolve_next()
                .expect("window A's request is the only one pending");
            let window_a = pending_a
                .try_take()
                .expect("resolve_next delivers synchronously")
                .expect("mock window creation cannot fail");
            assert!(
                Arc::ptr_eq(&resolved_a, &window_a),
                "resolve_next's own return value must be the SAME window it delivered \
                 through the PendingWindow"
            );

            let dispatcher_a =
                install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);
            window_a.on_close(Box::new(move || {
                close_this_window(dispatcher_a);
            }));

            // Window B: SharedRealm -- installs a second PRESENTATION of
            // realm A, never a second realm.
            let opened =
                open_secondary_window_impl(AppConfig::default(), WindowPolicy::SharedRealm)
                    .expect("the Pending arm must be accepted, not treated as an error");
            assert!(
                opened.is_none(),
                "a Pending open must return None -- the install completes asynchronously, \
                 not inline"
            );

            let presentation_count_before = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .realms
                    .get(&dispatcher_a.address.realm_id)
                    .and_then(|realm_slot| realm_slot.realm.as_ref())
                    .expect("realm A is resident")
                    .presentation_count()
            });
            assert_eq!(
                presentation_count_before, 1,
                "nothing must be installed for window B before its Pending open resolves"
            );

            // Resolve the deferred open -- delivers the mock window through
            // the ClaimSlot and hands the SAME window handle back here, so
            // this test can drive a REAL platform-level close on it later.
            let window_b = deferred
                .resolve_next()
                .expect("exactly one request (window B's) is pending at this point");

            // Drive the driver realm's (realm A's) own frame pump. THIS is
            // the exact call that used to panic (`debug_assert!` in
            // `install_presentation_alongside`) / return `DispatchInFlight`
            // (release builds) before the completion was deferred to this
            // dispatch's own tail instead of running inline from inside the
            // poll.
            dispatch_platform_realm(
                dispatcher_a,
                RealmTask::Frame(Box::new(|realm| {
                    realm.scheduler().drive_async_tasks();
                })),
            )
            .expect("realm A dispatches its own frame pump");

            assert_eq!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
                1,
                "SharedRealm must route into the SAME realm as the primary, never install a \
                 second one"
            );
            let presentation_count_after = APP_RUNTIME.with(|slot| {
                slot.borrow()
                    .realms
                    .get(&dispatcher_a.address.realm_id)
                    .and_then(|realm_slot| realm_slot.realm.as_ref())
                    .expect("realm A is still resident")
                    .presentation_count()
            });
            assert_eq!(
                presentation_count_after, 2,
                "the driver realm's own frame pump must have completed window B's presentation \
                 install -- if the deferred-completion queue is ever bypassed and \
                 finish_open_secondary_window runs directly from inside the poll again, this \
                 assertion is never reached (DispatchInFlight panics/errors first)"
            );

            // Close B through a REAL platform close (`resolve_next`'s own
            // return value, the SAME handle `finish_open_secondary_window`
            // wired) -- must remove only itself, never the shared realm.
            window_b.close();
            let (realm_count, presentation_count) = APP_RUNTIME.with(|slot| {
                let state = slot.borrow();
                let realm_count = state.realms.iter().count();
                let presentation_count = state
                    .realms
                    .get(&dispatcher_a.address.realm_id)
                    .and_then(|realm_slot| realm_slot.realm.as_ref())
                    .expect(
                        "the shared realm must still be resident -- B's own close must not \
                         have torn down the whole realm out from under the still-open primary",
                    )
                    .presentation_count();
                (realm_count, presentation_count)
            });
            assert_eq!(
                realm_count, 1,
                "the shared realm must survive B's own close"
            );
            assert_eq!(
                presentation_count, 1,
                "only B's own presentation must be removed; the primary's survives"
            );
            assert_eq!(
                quit_calls.load(Ordering::SeqCst),
                0,
                "the realm (and its primary presentation) still lives -- this must not exit"
            );

            window_a.close();
            assert_eq!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
                0,
                "closing the realm's last surviving presentation must uninstall the whole realm"
            );
            assert_eq!(
                quit_calls.load(Ordering::SeqCst),
                1,
                "closing the last presentation of the last realm must allow the exit exactly \
                 once"
            );

            Ok(())
        }));
        ready.expect("on_ready must not fail");

        teardown_platform_realm();
        drop(clear_guard);
    }

    /// Non-blocking correctness lead (not a merge-blocking finding): if the
    /// driver realm dies BEFORE its own `open_secondary_window` `Pending`
    /// request ever resolves, the fail-closed path must run cleanly. The
    /// spawned completion future is owned by the driver realm's own
    /// `UpdateScheduler`/`AsyncDriver`; tearing that realm down (its sole
    /// presentation closing) drops the `AsyncDriver`'s task map, which drops
    /// the future, which drops the `PendingWindow` it captured --
    /// `ClaimHandle`'s own disclaim-on-drop transitions the request to
    /// `Abandoned` (see `flui_foundation::claim_slot`'s module doc), never a
    /// panic or a wedged owner lane. `HeadlessDeferredWindowOpens::
    /// resolve_next`, called AFTER the realm is gone, still builds and hands
    /// back a window (nobody was left to deliver it to matters at the
    /// `ClaimSlot::deliver` level, traced as a debug message, not here) --
    /// what this test actually pins is that an abandoned request never
    /// zombie-installs a realm/presentation nobody asked for anymore.
    #[test]
    fn dead_driver_realm_before_pending_resolution_disclaims_cleanly_without_zombie_installing() {
        use flui_platform::traits::Platform;

        let clear_guard = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();

        let ready = Box::new(platform).run(Box::new(move |owner| {
            install_owner_platform(owner);

            let mut pending_a = match with_owner_platform(|owner| {
                owner.open_window(flui_platform::WindowOptions::default())
            })
            .expect("BUG: install_owner_platform just ran above")
            .expect("deferred mode still accepts the request")
            {
                flui_platform::WindowOpen::Pending(pending) => pending,
                flui_platform::WindowOpen::Ready(_) => {
                    panic!("deferred mode must return Pending, not Ready")
                }
            };
            let resolved_a = deferred
                .resolve_next()
                .expect("window A's request is the only one pending");
            let window_a = pending_a
                .try_take()
                .expect("resolve_next delivers synchronously")
                .expect("mock window creation cannot fail");
            assert!(Arc::ptr_eq(&resolved_a, &window_a));

            let dispatcher_a =
                install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);
            window_a.on_close(Box::new(move || {
                close_this_window(dispatcher_a);
            }));

            // Request window B -- Pending, spawns its completion onto realm
            // A's own AsyncDriver -- but never resolve it.
            let opened =
                open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                    .expect("the Pending arm must be accepted, not treated as an error");
            assert!(opened.is_none());

            // Kill the driver realm BEFORE window B's open ever resolves --
            // `window_a` is the realm's sole presentation, so this
            // uninstalls the whole realm, dropping its UpdateScheduler/
            // AsyncDriver and, with it, the still-pending completion
            // future.
            window_a.close();
            assert_eq!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
                0,
                "the driver realm must be gone before window B's open ever resolves"
            );

            // Resolving now must not panic: the ClaimSlot's `deliver` sees
            // the handle already abandoned (traced, discarded) --
            // and, crucially, this must never zombie-install a
            // realm/presentation nobody is left to claim.
            let resolved = deferred.resolve_next();
            assert!(
                resolved.is_some(),
                "resolve_next still builds and hands back the window even though the request \
                 was abandoned in the meantime -- abandonment is observed at the ClaimSlot \
                 level, not by resolve_next refusing to run"
            );
            assert_eq!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
                0,
                "an abandoned Pending request must never zombie-install a realm/presentation \
                 after the fact -- the completion future that would have done so was dropped \
                 along with the driver realm's own AsyncDriver, never polled again"
            );

            Ok(())
        }));
        ready.expect("on_ready must not fail");

        teardown_platform_realm();
        drop(clear_guard);
    }

    /// Closing a realm's SOLE presentation must dispatch `AppLifecycleState::
    /// Detached` through it BEFORE the realm is uninstalled — shutdown must
    /// cancel any in-flight pointer sequence and notify lifecycle observers
    /// before the realm and its `UpdateScheduler` are gone (the same reason
    /// `on_quit`'s own Detached dispatch exists, generalized to per-realm
    /// teardown so it fires from an ordinary window close too, not only a
    /// process-wide quit). Observed via an `UpdateScheduler` lifecycle listener
    /// registered BEFORE the close: the listener's own `Arc` survives the
    /// realm's eventual drop, so it remains checkable after `close_this_
    /// window` returns even though the realm itself is gone by then.
    #[test]
    fn closing_the_sole_presentation_dispatches_detached_before_uninstalling() {
        use std::sync::Mutex;

        let (dispatcher, _clear_guard) = install_realm_a_through_a_real_owner_platform();

        let observed_states: Arc<Mutex<Vec<AppLifecycleState>>> = Arc::new(Mutex::new(Vec::new()));
        let observed_states_for_listener = Arc::clone(&observed_states);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |realm| {
                realm
                    .scheduler()
                    .add_lifecycle_state_listener(Arc::new(move |state| {
                        observed_states_for_listener
                            .lock()
                            .expect("test-local mutex is never poisoned")
                            .push(state);
                    }));
            })),
        )
        .expect("registering the listener dispatches");

        close_this_window(dispatcher);

        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "closing the sole presentation must have uninstalled the whole realm"
        );
        assert_eq!(
            observed_states
                .lock()
                .expect("test-local mutex is never poisoned")
                .last(),
            Some(&AppLifecycleState::Detached),
            "the realm's own scheduler must have observed a transition to Detached before it \
             was torn down -- shutdown must cancel in-flight pointer sequences and notify \
             lifecycle observers before the realm is gone, not skip that step silently"
        );

        teardown_platform_realm();
    }

    /// KNOWN, TRACKED GAP — not fixed by this slice (see
    /// `app-runtime-composition-host`'s own residual note in
    /// `docs/runtime-contract.toml`). Calling the platform-level
    /// `window.close()` REENTRANTLY, from INSIDE a dispatched realm callback
    /// (as opposed to the ordinary path — a close arriving from OUTSIDE any
    /// active dispatch), makes the exit-policy hook consult a registry that
    /// has not actually emptied yet: `close_this_window`'s own
    /// `RealmTask::ClosePresentation` request (fired by `window_a`'s own
    /// registered `on_close`) is a SAME-REALM reentrant dispatch, which only
    /// enqueues onto the already-checked-out realm's queue rather than
    /// running synchronously — the actual uninstall only applies at the
    /// OUTER dispatch's own tail, strictly AFTER `window.close()`'s own
    /// `notify_closed` (and the hook it consulted) has already returned
    /// "don't exit". Nothing re-checks once the deferred uninstall finally
    /// applies, so the exit is missed, not merely delivered late. This test
    /// PINS the CURRENT (gap) behavior — it needs rewriting, not deleting,
    /// the day this is fixed (tracked follow-up: re-consult the hook once a
    /// deferred realm-map mutation actually applies, e.g. at each
    /// dispatch's own tail).
    #[test]
    fn closing_the_last_window_reentrantly_from_inside_a_dispatch_misses_the_exit_today() {
        use std::sync::atomic::Ordering;

        let (dispatcher, window_a, quit_calls, _clear_guard) =
            install_realm_a_with_exit_policy_and_quit_counter();

        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_realm| {
                // Reentrant: this window's own `on_close` (wired by the
                // helper above, mirroring `run_desktop`) calls
                // `close_this_window(dispatcher)` -> `close_presentation` ->
                // a nested `dispatch_platform_realm` on the SAME realm this
                // Frame task's own dispatch already checked out.
                window_a.close();
            })),
        )
        .expect("the outer Frame dispatch itself must not be refused");

        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0,
            "the deferred uninstall must still apply by the end of the outer dispatch's own \
             tail, even though the exit check that ran mid-dispatch missed it"
        );
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "KNOWN GAP: the exit-policy hook, consulted from inside window.close()'s own \
             notify_closed (itself called from a nested, same-realm dispatch), still sees the \
             registry as non-empty -- the uninstall it requested is only queued, applied at the \
             OUTER dispatch's own tail -- so it vetoes an exit that is, moments later, actually \
             correct. Nothing re-checks afterward. Fix tracked, not silently accepted as \
             intended behavior."
        );

        teardown_platform_realm();
    }

    /// The drain-before-decide rule: closing the splash realm and installing
    /// the main realm in the very same batch must never exit the loop --
    /// the surviving main realm keeps it alive. Drives the deferral through
    /// the REAL seam, `for_each_installed_realm`'s visit (hot-restart's own
    /// primitive), rather than hand-poking `iterating_all_realms` -- a
    /// splash's own dispose callback requesting the main window mid-visit is
    /// exactly this shape.
    #[test]
    fn close_splash_then_open_main_does_not_exit() {
        let splash = install_test_realm();
        let splash_id = splash.address.realm_id;

        let platform = flui_platform::headless_platform();
        let main_window = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create the main window");
        let main_realm = super::super::ui_realm::UiRealm::for_test();
        let main_id = main_realm.realm_id();
        let mut main_realm = Some(main_realm);

        for_each_installed_realm(|realm| {
            assert_eq!(
                realm.realm_id(),
                splash_id,
                "only the splash realm is resident at visit time"
            );
            uninstall_platform_realm(splash_id);
            let installed = install_realm_alongside(
                main_realm.take().expect("visited exactly once"),
                &main_window,
            );
            assert!(
                installed.is_ok(),
                "install_realm_alongside must defer mid-visit, not fail"
            );

            // Mid-visit: both requests just made are still queued (deferred
            // by `iterating_all_realms`), so the splash realm's own slot is
            // still resident (only marked for pending removal) and
            // `should_exit` must not report exit.
            let (exit, removed) = APP_RUNTIME.with(|slot| {
                slot.borrow_mut()
                    .should_exit(ExitPolicy::OnLastWindowClosed)
            });
            drop(removed);
            assert!(
                !exit,
                "should_exit must not report exit while this visit is still in flight"
            );
        });

        // Both deferred mutations land once the whole visit completes (its
        // own tail drain).
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&splash_id).is_none()),
            "the deferred splash uninstall must have applied once the visit completed"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&main_id).is_some()),
            "the deferred main-window install must have applied once the visit completed"
        );

        let should_exit = APP_RUNTIME.with(|slot| {
            let (exit, removed) = slot
                .borrow_mut()
                .should_exit(ExitPolicy::OnLastWindowClosed);
            drop(removed);
            exit
        });
        assert!(
            !should_exit,
            "the drain-before-decide rule: closing the splash and installing the main realm in \
             the same pass must never exit the loop -- the surviving main realm keeps it alive"
        );

        teardown_platform_realm();
    }

    /// A frame callback that decides to open a second window must install
    /// the second realm without ever attempting a nested dispatch: the
    /// install is invisible in the registry until realm A's own dispatch
    /// restores, at which point it lands.
    #[test]
    fn frame_callback_opening_a_window_installs_second_realm_without_nested_dispatch() {
        // Both windows from ONE shared platform instance -- see
        // `install_two_test_realms`'s doc for why two separate
        // `test_window()` calls would risk an aliased id here.
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let dispatcher_a =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_a);

        let second_realm = super::super::ui_realm::UiRealm::for_test();
        let second_realm_id = second_realm.realm_id();
        let second_window = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create the second window");
        let mut second_realm = Some(second_realm);

        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |_realm| {
                let installed = install_realm_alongside(
                    second_realm.take().expect("dispatched exactly once"),
                    &second_window,
                );
                assert!(
                    installed.is_ok(),
                    "install_realm_alongside must defer mid-dispatch (Ok, not yet visible), \
                     never fail"
                );
                assert!(
                    APP_RUNTIME.with(|slot| slot.borrow().realms.get(&second_realm_id).is_none()),
                    "the second realm must not be visible in the registry while realm A is \
                     still checked out for dispatch"
                );
            })),
        )
        .expect("A's frame callback dispatches");

        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&second_realm_id).is_some()),
            "the deferred install must land once realm A's own dispatch restores"
        );

        teardown_platform_realm();
    }

    /// Hot-restart's `for_each_installed_realm` visits every hosted realm in
    /// mount order; a realm-map mutation a visited realm's own callback
    /// requests must defer until the WHOLE visit completes, never change the
    /// set of realms still being walked.
    #[test]
    fn hot_restart_while_callback_mutates_realm_map_defers_mutation() {
        let (dispatcher_a, dispatcher_b) = install_two_test_realms();
        let realm_a_id = dispatcher_a.address.realm_id;
        let realm_b_id = dispatcher_b.address.realm_id;

        let visited = Rc::new(RefCell::new(Vec::new()));
        let visited_in_closure = Rc::clone(&visited);

        for_each_installed_realm(move |realm| {
            visited_in_closure.borrow_mut().push(realm.realm_id());
            if realm.realm_id() == realm_a_id {
                uninstall_platform_realm(realm_a_id);
                assert!(
                    APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_some()),
                    "a mutation requested mid-visit must defer, not remove the realm immediately"
                );
            }
        });

        assert_eq!(
            *visited.borrow(),
            vec![realm_a_id, realm_b_id],
            "the visit must walk every realm in mount order, unaffected by the mid-visit \
             mutation request"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_a_id).is_none()),
            "the deferred uninstall must apply once the whole visit completes"
        );
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_b_id).is_some()),
            "realm B must be untouched by realm A's own deferred mutation"
        );

        teardown_platform_realm();
    }

    /// `should_exit` called from mid-visit must not drain a mutation the
    /// SAME visit just deferred. Before this guard existed,
    /// `drain_pending_realm_mutations` drained unconditionally, so this call
    /// would have applied the visited realm's own queued self-uninstall
    /// immediately -- removing its slot from the registry while its
    /// `UiRealm` is still checked out here, held by `for_each_installed_realm`'s
    /// loop, about to be restored into a slot that would no longer exist.
    #[test]
    fn should_exit_mid_visit_does_not_prematurely_drain_the_visited_realms_own_pending_uninstall() {
        let dispatcher = install_test_realm();
        let realm_id = dispatcher.address.realm_id;

        for_each_installed_realm(|realm| {
            assert_eq!(
                realm.realm_id(),
                realm_id,
                "the sole installed realm is the one visited"
            );
            uninstall_platform_realm(realm_id);

            let (exit, removed) = APP_RUNTIME.with(|slot| {
                slot.borrow_mut()
                    .should_exit(ExitPolicy::OnLastWindowClosed)
            });
            drop(removed);
            assert!(
                !exit,
                "the registry is not empty: this realm's slot is still resident here, only its \
                 checked-out UiRealm is held externally by this visit"
            );
            assert!(
                APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_some()),
                "should_exit called mid-visit must not drain a mutation the SAME visit just \
                 deferred -- draining it now would remove this realm's slot while its UiRealm \
                 is still checked out here, and the visit's own restore step would then \
                 silently drop it instead of the queued uninstall ever running its own \
                 registry cleanup"
            );
        });

        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_none()),
            "the deferred uninstall must still apply once the whole visit completes"
        );

        teardown_platform_realm();
    }

    /// A panicking visitor (e.g. hot-restart's reassemble running arbitrary
    /// user build code) must not permanently strand the checked-out realm
    /// at `realm: None`, nor wedge `iterating_all_realms` at `true` forever
    /// -- both of which would (after the fix above) silently defer every
    /// future realm-map mutation request for the rest of the process.
    #[test]
    fn panicking_visit_restores_the_checked_out_realm_and_clears_iterating_all_realms() {
        let dispatcher = install_test_realm();

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for_each_installed_realm(|_realm| {
                panic!("simulated visitor panic");
            });
        }));
        assert!(
            panic.is_err(),
            "the visitor's panic must propagate out of for_each_installed_realm, not be \
             swallowed"
        );

        assert!(
            APP_RUNTIME.with(|slot| !slot.borrow().iterating_all_realms),
            "a panicking visit must clear iterating_all_realms before its own panic resumes -- \
             otherwise every future realm-map mutation request would defer forever"
        );

        // A bare `.expect(Ok(()))` on the dispatch below is NOT sufficient
        // evidence the realm's slot was actually restored: if `realm: None`
        // were left stranded, `dispatch_platform_realm` would take its
        // enqueue-only early-return path (`realm_slot.realm.is_none()`) and
        // still return `Ok(())` — successfully queuing the task forever
        // without ever running it, indistinguishable from success at the
        // `Result` level alone. Give the task an observable side effect
        // instead: it only runs SYNCHRONOUSLY, inside this same call, if the
        // slot's `realm` was genuinely `Some(..)` at call time.
        let ran = Rc::new(Cell::new(false));
        let ran_in_task = Rc::clone(&ran);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_| {
                ran_in_task.set(true);
            })),
        )
        .expect("dispatch must return cleanly after a panicking visit");
        assert!(
            ran.get(),
            "the dispatched task must actually RUN, not merely enqueue forever -- it only runs \
             if the visit's cleanup genuinely restored realm: Some(..) into this realm's slot; \
             a stranded realm: None slot would still return Ok(()) from the enqueue-only path \
             above without ever executing this closure"
        );

        teardown_platform_realm();
    }

    /// Fence-(c) two-realm probe: with realm A mid-frame-transaction and a
    /// SECOND, fully idle realm B also resident, `with_owner_platform`'s
    /// debug_assert must still trip. A two-realm registry is not an "OR of
    /// all Idle" check that a lone idle sibling could vacuously satisfy —
    /// this must actually catch the realm that IS mid-transaction.
    #[test]
    #[should_panic(
        expected = "with_owner_platform called while the installed realm's scheduler is inside"
    )]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the fence is a debug_assert!; release builds don't panic"
    )]
    fn owner_platform_accessor_fences_correctly_with_a_second_idle_realm_present() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        // Both windows from ONE shared platform instance: `HeadlessPlatform`
        // mints window ids from an instance-local counter, so two separate
        // `headless_platform()` calls could alias the same id, which would
        // silently displace realm A's window mapping the moment realm B's
        // window registers.
        let platform = headless_platform();
        // The IDLE sibling installs FIRST (`install_platform_realm`, this
        // test's legacy-primary slot), the mid-transaction realm SECOND
        // (`install_realm_alongside`) -- deliberately the opposite of mount
        // order elsewhere in this file. `installed_realm_phase` falls back to
        // `phases.first()` when nothing scans as forbidden; installing the
        // idle realm first means that fallback alone would return `Idle` and
        // this test would pass FALSELY (the debug_assert would never trip,
        // failing `#[should_panic]`) if the `.find(is_frame_transaction_phase)`
        // scan were ever deleted. Only the real scan, checking every
        // resident slot rather than assuming the first one is representative,
        // makes this test kill that mutant.
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");
        // Realm B is installed and then never touched again for the rest of
        // this test -- it stays resident and Idle throughout.
        let _dispatcher_b =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window_b);
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let dispatcher_a =
            install_realm_alongside(super::super::ui_realm::UiRealm::for_test(), &window_a)
                .expect("realm A installs alongside the idle realm B cleanly");

        let (scheduler_a, local_post_frame_a) = APP_RUNTIME.with(|slot| {
            let borrowed = slot.borrow();
            let realm = borrowed
                .realms
                .get(&dispatcher_a.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("realm A installed above");
            (
                realm.scheduler().clone(),
                realm.local_post_frame_lane().clone(),
            )
        });

        let now = web_time::Instant::now();
        scheduler_a.drive_frame_with_lane(
            now,
            flui_scheduler::IdleDeadline::far_future(now),
            || {
                let _ = with_owner_platform(|_owner| ());
            },
            &local_post_frame_a,
        );
    }

    /// The presentation-close dispatch seam (`close_presentation` ->
    /// `dispatch_platform_realm` -> the special-cased `RealmTask::
    /// ClosePresentation` branch -> `UiRealm::close_presentation_entered`)
    /// runs presentation A's teardown with the exact same
    /// `dispatched_realm_id` TLS state any other dispatched frame/event
    /// callback runs under. One dispose hook on A proves both halves of
    /// that claim:
    ///
    /// 1. it can RESOLVE a `GlobalKey` registered in SIBLING presentation
    ///    B — the whole-frame composite `close_presentation_entered`'s own
    ///    `self.enter(...)` activates for its step 2–3 phase spans every
    ///    presentation still in the forest, not just the one being closed
    ///    (the same fact `whole_frame_event_keeps_realm_global_key_scope_active`
    ///    proves for an ordinary dispatched frame);
    /// 2. an `install_realm_alongside` call it makes must DEFER — not
    ///    apply immediately, not reenter `APP_RUNTIME` — exactly the
    ///    discipline `frame_callback_opening_a_window_installs_second_realm_without_nested_dispatch`
    ///    proves for an ordinary dispatched frame callback.
    ///
    /// Red-check performed by hand while writing this test (not left in the
    /// tree): replacing the `close_presentation(dispatcher, a_id)` call
    /// below with a direct `realm.close_presentation_entered(a_id)` reached
    /// through a raw `APP_RUNTIME` borrow-and-take (bypassing
    /// `dispatch_platform_realm` entirely, so `dispatched_realm_id` stays
    /// `None` for the whole teardown) makes the `install_deferred`
    /// assertion below fail: with no dispatch in flight,
    /// `install_realm_alongside` applies the new realm IMMEDIATELY instead
    /// of deferring, so this test observes it already installed at the
    /// point it checks — proving the assertion is genuinely pinned to the
    /// TLS deferral guard, not vacuously green regardless of which path
    /// teardown takes.
    #[test]
    fn dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter() {
        #[derive(Clone)]
        struct OpenWindowOnDisposeView {
            key_in_sibling: flui_view::GlobalKey<()>,
            new_window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
            resolved_sibling_element: Rc<Cell<Option<flui_foundation::ElementId>>>,
            install_deferred: Rc<Cell<Option<bool>>>,
            opened_realm_id: Rc<Cell<Option<flui_foundation::RealmId>>>,
        }

        struct OpenWindowOnDisposeState {
            key_in_sibling: flui_view::GlobalKey<()>,
            new_window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
            resolved_sibling_element: Rc<Cell<Option<flui_foundation::ElementId>>>,
            install_deferred: Rc<Cell<Option<bool>>>,
            opened_realm_id: Rc<Cell<Option<flui_foundation::RealmId>>>,
        }

        impl flui_view::StatefulView for OpenWindowOnDisposeView {
            type State = OpenWindowOnDisposeState;

            fn create_state(&self) -> Self::State {
                OpenWindowOnDisposeState {
                    key_in_sibling: self.key_in_sibling.clone(),
                    new_window: std::sync::Arc::clone(&self.new_window),
                    resolved_sibling_element: Rc::clone(&self.resolved_sibling_element),
                    install_deferred: Rc::clone(&self.install_deferred),
                    opened_realm_id: Rc::clone(&self.opened_realm_id),
                }
            }
        }

        impl flui_view::ViewState<OpenWindowOnDisposeView> for OpenWindowOnDisposeState {
            fn build(
                &self,
                _view: &OpenWindowOnDisposeView,
                _ctx: &dyn flui_view::BuildContext,
            ) -> impl flui_view::IntoView {
                flui_widgets::SizedBox::new(0.0, 0.0)
            }

            fn dispose(&mut self) {
                // (1) Resolve a GlobalKey registered in the SIBLING
                // presentation B: only possible if the whole-frame composite
                // is still active for this dispose call, spanning every
                // OTHER presentation the realm hosts (this presentation
                // itself, mid-teardown, is deliberately excluded from that
                // composite -- see `UiRealm::enter_for_close`'s doc).
                self.resolved_sibling_element
                    .set(self.key_in_sibling.current_element());

                // (2) Attempt to open a second realm/window mid-teardown.
                // Production contract: this must defer to loop idle, the
                // same as any other dispatched callback's install request.
                // `new_window` was pre-opened from the SAME shared platform
                // instance realm A's own window came from -- a second,
                // independently-constructed `HeadlessPlatform` here would
                // mint an aliasing window id from its own zeroed counter,
                // which `WindowRegistry` would then read as a collision
                // against A's window rather than a genuinely distinct one.
                let new_realm = super::super::ui_realm::UiRealm::for_test();
                let new_realm_id = new_realm.realm_id();
                let installed = install_realm_alongside(new_realm, &self.new_window);
                assert!(
                    installed.is_ok(),
                    "install_realm_alongside must defer mid-teardown (Ok, not yet visible), \
                     never fail"
                );
                self.opened_realm_id.set(Some(new_realm_id));
                let visible_immediately =
                    APP_RUNTIME.with(|slot| slot.borrow().realms.get(&new_realm_id).is_some());
                self.install_deferred.set(Some(!visible_immediately));
            }
        }

        impl View for OpenWindowOnDisposeView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateful(self)
            }
        }

        // One shared platform instance for every window -- see
        // `install_two_test_realms`'s doc for why two independent
        // `headless_platform()` calls would risk an aliased window id.
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");
        let window_for_new_realm = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create the second window");

        let realm = super::super::ui_realm::UiRealm::for_test();
        let a_id = realm.presentation_id();

        let key_in_sibling = flui_view::GlobalKey::<()>::new();
        let element_in_sibling = flui_foundation::ElementId::new(1);

        let resolved_sibling_element = Rc::new(Cell::new(None));
        let install_deferred = Rc::new(Cell::new(None));
        let opened_realm_id = Rc::new(Cell::new(None));
        let probe = OpenWindowOnDisposeView {
            key_in_sibling: key_in_sibling.clone(),
            new_window: window_for_new_realm,
            resolved_sibling_element: Rc::clone(&resolved_sibling_element),
            install_deferred: Rc::clone(&install_deferred),
            opened_realm_id: Rc::clone(&opened_realm_id),
        };
        realm
            .enter(|realm| realm.attach_root_widget(&probe))
            .expect("A mounts the probe");
        // `attach_root_widget` only SCHEDULES the initial build; the probe's
        // concrete element (and therefore its `State`) is not actually
        // constructed until a frame runs it, exactly the same reason
        // `dispose_during_teardown_cannot_reach_sibling_or_dead_services`
        // and `closing_presentation_a_leaves_sibling_layer_tree_identical`
        // both draw a frame between mounting and closing.
        let _ = realm.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
            flui_types::Size::new(
                flui_types::geometry::px(20.0),
                flui_types::geometry::px(20.0),
            ),
        ));

        let dispatcher = install_platform_realm(realm, &window_a);
        // B installs alongside A through the REAL production entry point
        // (issue #555's addressed-routing slice), with its own genuine
        // `WindowRegistry` mapping -- not the `cfg(test)`-only
        // `install_second_presentation_for_test` bypass this test used
        // before, which left B with no mapping of its own and is why
        // `teardown_platform_realm()` used to have to be skipped here.
        let b_dispatcher = install_presentation_alongside(dispatcher, &window_b)
            .expect("B installs alongside A with a real window mapping");
        let b_id = b_dispatcher.address.presentation_id;

        // Register the sibling key into B directly: no dispatch is in
        // flight yet (registration itself needs no active composite; only
        // resolution via `GlobalKey::current_element` does, which is what
        // A's dispose hook below exercises).
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            let realm_slot = state
                .realms
                .get(&dispatcher.address.realm_id)
                .expect("installed above");
            let realm = realm_slot.realm.as_ref().expect("not checked out yet");
            realm
                .presentation_widgets_for_test(b_id)
                .with_build_owner_mut(|owner| {
                    owner.register_global_key(key_in_sibling.id(), element_in_sibling);
                });
        });

        close_presentation(dispatcher, a_id).expect("A closes through the real dispatch seam");

        assert_eq!(
            resolved_sibling_element.get(),
            Some(element_in_sibling),
            "A's dispose hook must resolve B's GlobalKey through the whole-frame composite \
             active during close_presentation_entered's step 2-3 phase"
        );
        assert_eq!(
            install_deferred.get(),
            Some(true),
            "A's dispose-time install_realm_alongside request must defer -- not yet visible \
             in the registry -- while A's own realm is still checked out for this dispatch"
        );

        let opened_realm_id = opened_realm_id
            .get()
            .expect("dispose must have attempted the install");
        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&opened_realm_id).is_some()),
            "the deferred install must land once A's close dispatch restores"
        );

        // B now has a real `WindowRegistry` mapping of its own (minted by
        // `install_presentation_alongside` above), so after A closes and the
        // surviving realm's tracked address re-stamps to B, teardown's own
        // self-check ("every live realm's tracked address resolves to a
        // registered window") holds again -- the still-open design-for-N gap
        // this test used to skip teardown for is closed.
        teardown_platform_realm();
    }

    /// Closing presentation A (a sibling, B, stays open) must unregister
    /// EXACTLY A's own window mapping -- and a caller still holding a
    /// `RealmDispatcher` minted while A was live must be refused, never
    /// silently delivered to B (the surviving primary) instead. Before the
    /// `ClosePresentation` handling did both the window-registry removal
    /// AND the `RealmSlot::address` re-stamp, a stale dispatcher would still
    /// compare equal at the top of `dispatch_platform_realm` (the slot's
    /// tracked `presentation_id` was never updated past A's original
    /// install-time value) and the task would run against `self.
    /// presentations.primary()` -- B, after A's removal shifts it into that
    /// slot -- exactly the sibling-reach bug this test guards.
    #[test]
    fn closing_a_presentation_unregisters_its_window_mapping() {
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");

        let realm = super::super::ui_realm::UiRealm::for_test();
        let a_id = realm.presentation_id();

        let dispatcher = install_platform_realm(realm, &window_a);
        let realm_id = dispatcher.address.realm_id;
        // B installs alongside A through the real production entry point
        // (issue #555's addressed-routing slice), with its own genuine `WindowRegistry` mapping.
        let b_dispatcher = install_presentation_alongside(dispatcher, &window_b)
            .expect("B installs alongside A with a real window mapping");
        let b_id = b_dispatcher.address.presentation_id;

        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_a.id())),
            Some(flui_foundation::PresentationAddress {
                realm_id,
                presentation_id: a_id,
            }),
            "precondition: A's window starts mapped to A's own address"
        );
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_b.id())),
            Some(b_dispatcher.address),
            "precondition: B's window starts mapped to B's own address"
        );

        close_presentation(dispatcher, a_id).expect("A closes through the real dispatch seam");

        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_a.id())),
            None,
            "closing A must unregister its own window mapping -- leaving it mapped would let \
             a stale platform event for that exact window keep resolving to A's now-dead \
             PresentationId"
        );
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().registry.resolve(window_b.id())),
            Some(b_dispatcher.address),
            "closing A must never touch B's own window mapping -- hop-1 demux is per-window, \
             not per-realm"
        );

        let delivered = Rc::new(Cell::new(false));
        let delivered_in_task = Rc::clone(&delivered);
        let result = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |_realm| {
                delivered_in_task.set(true);
            })),
        );
        assert!(
            matches!(result, Err(RealmDispatchError::StalePresentation)),
            "a dispatcher minted for the now-closed presentation must be refused as stale, \
             got {result:?}"
        );
        assert!(
            !delivered.get(),
            "the stale task must never run -- delivering it would land on B (the surviving \
             primary), exactly the sibling-reach bug this test guards"
        );

        // B has a real `WindowRegistry` mapping of its own (minted by
        // `install_presentation_alongside` above), so teardown's own
        // self-check ("every live realm's tracked address resolves to a
        // registered window") holds even after A closes and the surviving
        // realm's tracked address re-stamps to B -- the still-open
        // design-for-N gap this test used to skip teardown for is closed.
        teardown_platform_realm();
        let _ = b_id;
    }

    /// A dispose hook that re-enters `dispatch_platform_realm` DURING the
    /// very close it is being disposed by -- using the dying presentation's
    /// OWN dispatcher -- must be refused as stale
    /// (`RealmDispatchError::StalePresentation`), never silently ACCEPTED
    /// and enqueued to run later against whatever survives the close.
    ///
    /// Ordering-critical: `RealmSlot::address` must re-stamp to the
    /// surviving primary BEFORE this presentation's own dispose hooks run
    /// (`close_presentation_entered`'s step 2-3), not after. Re-stamping
    /// only after disposal would leave a window where a dispose-time
    /// reentrant dispatch bearing `id` still compares EQUAL against the
    /// not-yet-updated address, passes the `StalePresentation` check, and
    /// gets enqueued behind the same-realm reentrancy guard (`realm_slot.
    /// draining` is `true` for the whole checkout) instead of refused --
    /// only to run a moment later, in this very drain loop, against
    /// whichever presentation survives: a task genuinely executing against
    /// the wrong presentation, not merely an error a caller has to notice.
    #[test]
    fn dispose_time_reentrant_dispatch_with_the_dying_presentations_own_dispatcher_is_refused() {
        #[derive(Clone)]
        struct ReenterOnDisposeView {
            dispatcher: Rc<Cell<Option<RealmDispatcher>>>,
            reentrant_result: Rc<Cell<Option<Result<(), RealmDispatchError>>>>,
            reentrant_task_ran: Rc<Cell<bool>>,
        }

        struct ReenterOnDisposeState {
            dispatcher: Rc<Cell<Option<RealmDispatcher>>>,
            reentrant_result: Rc<Cell<Option<Result<(), RealmDispatchError>>>>,
            reentrant_task_ran: Rc<Cell<bool>>,
        }

        impl flui_view::StatefulView for ReenterOnDisposeView {
            type State = ReenterOnDisposeState;

            fn create_state(&self) -> Self::State {
                ReenterOnDisposeState {
                    dispatcher: Rc::clone(&self.dispatcher),
                    reentrant_result: Rc::clone(&self.reentrant_result),
                    reentrant_task_ran: Rc::clone(&self.reentrant_task_ran),
                }
            }
        }

        impl flui_view::ViewState<ReenterOnDisposeView> for ReenterOnDisposeState {
            fn build(
                &self,
                _view: &ReenterOnDisposeView,
                _ctx: &dyn flui_view::BuildContext,
            ) -> impl flui_view::IntoView {
                flui_widgets::SizedBox::new(0.0, 0.0)
            }

            fn dispose(&mut self) {
                let dispatcher = self
                    .dispatcher
                    .get()
                    .expect("dispatcher stashed before A's close was requested");
                let ran = Rc::clone(&self.reentrant_task_ran);
                let result = dispatch_platform_realm(
                    dispatcher,
                    RealmTask::Frame(Box::new(move |_realm| {
                        ran.set(true);
                    })),
                );
                self.reentrant_result.set(Some(result));
            }
        }

        impl View for ReenterOnDisposeView {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateful(self)
            }
        }

        // One shared platform instance for both windows -- see
        // `install_two_test_realms`'s doc for why two independent
        // `headless_platform()` calls would risk an aliased window id.
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");

        let realm = super::super::ui_realm::UiRealm::for_test();
        let a_id = realm.presentation_id();

        let dispatcher_slot = Rc::new(Cell::new(None));
        let reentrant_result = Rc::new(Cell::new(None));
        let reentrant_task_ran = Rc::new(Cell::new(false));
        let probe = ReenterOnDisposeView {
            dispatcher: Rc::clone(&dispatcher_slot),
            reentrant_result: Rc::clone(&reentrant_result),
            reentrant_task_ran: Rc::clone(&reentrant_task_ran),
        };
        realm
            .enter(|realm| realm.attach_root_widget(&probe))
            .expect("A mounts the probe");
        // `attach_root_widget` only SCHEDULES the initial build -- see
        // `dispose_opening_a_window_mid_teardown_defers_and_does_not_reenter`'s
        // own comment for why a real frame must run before the probe's
        // `State`, and therefore its `dispose()`, actually exists.
        let _ = realm.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
            flui_types::Size::new(
                flui_types::geometry::px(20.0),
                flui_types::geometry::px(20.0),
            ),
        ));

        let dispatcher = install_platform_realm(realm, &window_a);
        // B installs alongside A through the real production entry point
        // (issue #555's addressed-routing slice), with its own genuine `WindowRegistry` mapping.
        let _b_dispatcher = install_presentation_alongside(dispatcher, &window_b)
            .expect("B installs alongside A with a real window mapping");
        dispatcher_slot.set(Some(dispatcher));

        close_presentation(dispatcher, a_id).expect("A closes through the real dispatch seam");

        assert!(
            matches!(
                reentrant_result.get(),
                Some(Err(RealmDispatchError::StalePresentation))
            ),
            "a dispose-time re-dispatch bearing the dying presentation's OWN dispatcher must \
             be refused as stale, got {:?}",
            reentrant_result.get()
        );
        assert!(
            !reentrant_task_ran.get(),
            "the reentrant task must never run -- accepting it (even merely queued for later) \
             means it eventually executes against whatever survives the close, misaddressed \
             rather than refused"
        );

        // B has a real `WindowRegistry` mapping of its own (minted by
        // `install_presentation_alongside` above), so teardown's own
        // self-check holds even after A closes and the surviving realm's
        // tracked address re-stamps to B -- see
        // `closing_a_presentation_unregisters_its_window_mapping`'s closing
        // comment for the full explanation of the gap this closes.
        teardown_platform_realm();
    }

    /// Closing a realm's SOLE presentation must not leave a live realm with
    /// an empty `PresentationForest` (the next `primary()` call would panic
    /// `"BUG: PresentationForest is never empty"`). `RealmTask::
    /// ClosePresentation`'s handling routes this case into a full realm
    /// uninstall instead of ever calling `close_presentation_entered` --
    /// proven here by asserting the WHOLE realm disappears from the
    /// registry, not just the one presentation.
    #[test]
    fn closing_the_sole_presentation_uninstalls_the_whole_realm() {
        let dispatcher = install_test_realm();
        let realm_id = dispatcher.address.realm_id;
        let a_id = dispatcher.address.presentation_id;

        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_some()),
            "precondition: the realm is installed before closing its sole presentation"
        );

        close_presentation(dispatcher, a_id).expect("the close request is accepted");

        assert!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.get(&realm_id).is_none()),
            "closing the realm's only presentation must uninstall the WHOLE realm -- a \
             live realm with an empty forest is not a reachable state this codebase permits"
        );

        // Harmless no-op cleanup (the realm is already gone) -- matches
        // every other test in this module for the same TLS-state hygiene.
        teardown_platform_realm();
    }

    /// `FocusCoordinator`'s two write-side behaviors, end to end through the
    /// REAL production path (`dispatch_platform_realm` running a genuine
    /// `RealmTask::Event(PlatformToUi::WindowFocus(_))` task, not a direct
    /// `notify_presentation_focus_gained` call): `WindowFocus(true)`
    /// dispatched for a presentation moves the active presentation to it;
    /// `WindowFocus(false)` dispatched for a DIFFERENT, non-active
    /// presentation never moves it away from whichever presentation is
    /// currently active, and must NOT suspend the realm-wide lifecycle
    /// either (a sibling presentation still holds focus). The FINAL step
    /// pins the other half of that same guard: `WindowFocus(false)`
    /// dispatched for the presentation that IS currently active must still
    /// suspend the realm normally -- the guard drops only NON-active
    /// focus-loss, never focus-loss in general.
    ///
    /// Mutants, both confirmed to make this fail:
    /// - `PlatformToUi::run`'s `WindowFocus` arm calling
    ///   `notify_presentation_focus_gained` inside `if focused { .. }`
    ///   unconditionally (removing that guard, so `WindowFocus(false)` also
    ///   moves active) -- the active-presentation assertion after B gains
    ///   focus would observe A instead of B.
    /// - The polarity mutant `if !focused { return; }` in place of
    ///   `if !focused && !realm.is_active_presentation(presentation_id) { return; }`
    ///   -- dropping EVERY focus loss unconditionally, so the realm can
    ///   never suspend at all. This one is invisible to the middle
    ///   assertions (A's focus loss is already supposed to be dropped) and
    ///   is caught ONLY by the final step below, where B -- the ACTUALLY
    ///   active presentation -- loses focus and the realm must still
    ///   suspend.
    #[test]
    fn window_focus_true_moves_active_presentation_end_to_end_and_false_does_not() {
        // One shared platform instance for both windows -- see
        // `install_two_test_realms`'s doc for why two independent
        // `headless_platform()` calls would risk an aliased window id.
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");

        let realm = super::super::ui_realm::UiRealm::for_test();
        let dispatcher_a = install_platform_realm(realm, &window_a);
        let a_id = dispatcher_a.address.presentation_id;
        let dispatcher_b = install_presentation_alongside(dispatcher_a, &window_b)
            .expect("B installs alongside A with a real window mapping");
        let b_id = dispatcher_b.address.presentation_id;

        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.active_presentation_for_test(),
                    a_id,
                    "precondition: a freshly installed realm starts with its primary active"
                );
            })),
        )
        .expect("precondition frame task dispatches");

        // WindowFocus(true) dispatched for B, through the REAL dispatch
        // seam -- not a direct notify_presentation_focus_gained call.
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Event(PlatformToUi::WindowFocus(true)),
        )
        .expect("WindowFocus(true) for B dispatches");
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.active_presentation_for_test(),
                    b_id,
                    "WindowFocus(true) dispatched through the real production path must move \
                     the active presentation to the presentation it was addressed to"
                );
            })),
        )
        .expect("frame task dispatches");

        // WindowFocus(false) dispatched for A -- NOT the currently active
        // presentation (B is) -- must never move active away from B.
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Event(PlatformToUi::WindowFocus(false)),
        )
        .expect("WindowFocus(false) for A dispatches");
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.active_presentation_for_test(),
                    b_id,
                    "WindowFocus(false) must never move the active presentation, regardless \
                     of which presentation it was addressed to"
                );
                // The realm-wide lifecycle aggregate must ALSO stay
                // Resumed: A's focus loss is a normal consequence of focus
                // having already moved to B (a sibling presentation of
                // this SAME realm), not a sign that the realm itself lost
                // focus -- B still holds it. Before the fix, this
                // non-active-presentation `WindowFocus(false)` flipped the
                // still loop-scoped `focused` aggregate unconditionally,
                // incorrectly suspending the whole realm while B was
                // still focused.
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Resumed,
                    "a WindowFocus(false) from a non-active presentation must never suspend \
                     the realm while a sibling presentation still holds focus"
                );
            })),
        )
        .expect("frame task dispatches");

        // WindowFocus(false) dispatched for B -- the presentation that IS
        // currently active -- pins the OTHER half of the guard: dropping
        // non-active focus-loss must never widen into dropping ALL
        // focus-loss. The realm must actually suspend here.
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Event(PlatformToUi::WindowFocus(false)),
        )
        .expect("WindowFocus(false) for B dispatches");
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_ne!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Resumed,
                    "WindowFocus(false) from the presentation that IS currently active must \
                     still suspend the realm -- the guard drops only NON-active focus-loss, \
                     never focus-loss unconditionally"
                );
            })),
        )
        .expect("frame task dispatches");

        teardown_platform_realm();
    }

    /// `PlatformToUi::WindowVisibility` (hidden-surface gating) is
    /// per-presentation, addressed to exactly the presentation
    /// that produced it — never the realm-wide aggregate alone. Driven
    /// through the REAL dispatch seam (`dispatch_platform_realm`), not a
    /// direct `UiRealm::set_presentation_hidden` call, so this exercises
    /// `PlatformToUi::run`'s actual `WindowVisibility` arm.
    ///
    /// Mutant, confirmed to make this fail: removing
    /// `realm.set_presentation_hidden(presentation_id, !visible)` from that
    /// arm (leaving only the pre-existing loop-scoped `AppLifecycleState`
    /// derivation) -- both presentations' clocks would stay unhidden
    /// regardless of which one the event was addressed to.
    #[test]
    fn window_visibility_gates_exactly_the_addressed_presentations_clock() {
        let platform = flui_platform::headless_platform();
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_b = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window b");

        let realm = super::super::ui_realm::UiRealm::for_test();
        let dispatcher_a = install_platform_realm(realm, &window_a);
        let a_id = dispatcher_a.address.presentation_id;
        let dispatcher_b = install_presentation_alongside(dispatcher_a, &window_b)
            .expect("B installs alongside A with a real window mapping");
        let b_id = dispatcher_b.address.presentation_id;

        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.presentation_hidden_for_test(a_id),
                    Some(false),
                    "precondition: neither presentation starts hidden"
                );
                assert_eq!(realm.presentation_hidden_for_test(b_id), Some(false));
            })),
        )
        .expect("precondition frame task dispatches");

        // WindowVisibility(false) (occluded) dispatched for B only.
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Event(PlatformToUi::WindowVisibility(false)),
        )
        .expect("WindowVisibility(false) for B dispatches");
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.presentation_hidden_for_test(b_id),
                    Some(true),
                    "WindowVisibility(false) dispatched through the real production path must \
                     hide exactly the presentation it was addressed to"
                );
                assert_eq!(
                    realm.presentation_hidden_for_test(a_id),
                    Some(false),
                    "a sibling presentation's own clock must be untouched by an event \
                     addressed to a DIFFERENT presentation"
                );
            })),
        )
        .expect("frame task dispatches");

        // WindowVisibility(true) (unoccluded) dispatched for B undoes it,
        // still leaving A untouched.
        dispatch_platform_realm(
            dispatcher_b,
            RealmTask::Event(PlatformToUi::WindowVisibility(true)),
        )
        .expect("WindowVisibility(true) for B dispatches");
        dispatch_platform_realm(
            dispatcher_a,
            RealmTask::Frame(Box::new(move |realm| {
                assert_eq!(
                    realm.presentation_hidden_for_test(b_id),
                    Some(false),
                    "WindowVisibility(true) must unhide exactly the presentation it was \
                     addressed to"
                );
                assert_eq!(realm.presentation_hidden_for_test(a_id), Some(false));
            })),
        )
        .expect("frame task dispatches");

        teardown_platform_realm();
    }

    /// If `window`'s id is already registered (forced here by reusing A's
    /// OWN window as the "second" presentation's window), the forest must
    /// be left EXACTLY as it was -- no presentation resident without a
    /// registry mapping of its own (`single-native-window-map-authority`).
    ///
    /// If reverted (installing into the forest BEFORE registering, with no
    /// rollback on failure -- the pre-fix ordering): this fails --
    /// `presentation_count()` observes `2` instead of `1`, a forest-resident
    /// presentation this test's own colliding window never actually mapped.
    #[test]
    fn install_presentation_alongside_leaves_the_forest_unchanged_on_registration_failure() {
        let window_a = test_window();
        let realm = super::super::ui_realm::UiRealm::for_test();
        let dispatcher = install_platform_realm(realm, &window_a);

        // Reusing A's own window forces `try_register_window` to fail --
        // its id is already mapped to A's own address.
        let result = install_presentation_alongside(dispatcher, &window_a);
        assert!(
            matches!(
                result,
                Err(InstallPresentationError::WindowAlreadyMapped(_))
            ),
            "a colliding window id must be refused as WindowAlreadyMapped, got {result:?}"
        );

        let count = Rc::new(Cell::new(None));
        let count_in_task = Rc::clone(&count);
        dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(move |realm| {
                count_in_task.set(Some(realm.presentation_count()));
            })),
        )
        .expect("frame task dispatches");
        assert_eq!(
            count.get(),
            Some(1),
            "a registration failure must leave the forest exactly as it was -- the \
             assembled-but-unregistered presentation must never have been installed"
        );

        teardown_platform_realm();
    }

    /// A `RealmDispatcher` for a presentation that has since CLOSED must be
    /// refused, even though its realm survives (a sibling presentation kept
    /// it alive) -- the same authorization `dispatch_platform_realm`
    /// requires of every dispatched task. Before this fix,
    /// `install_presentation_alongside` checked only realm existence, so a
    /// caller still holding A's dispatcher after A closed could use it to
    /// install a THIRD presentation alongside the survivor.
    ///
    /// If reverted (the `registry.contains_address` check removed): this
    /// fails -- the install succeeds instead of returning
    /// `StalePresentation`, and `presentation_count()` observes `2` instead
    /// of `1`.
    #[test]
    fn install_presentation_alongside_refuses_a_stale_dispatcher_even_though_its_realm_survives() {
        // One shared platform instance for every window -- see
        // `install_two_test_realms`'s doc for why two independent
        // `headless_platform()` calls would risk an aliased window id.
        let platform = flui_platform::headless_platform();
        let window_p = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window p");
        let window_a = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window a");
        let window_c = platform
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create window c");

        let realm = super::super::ui_realm::UiRealm::for_test();
        let dispatcher_p = install_platform_realm(realm, &window_p);
        let dispatcher_a = install_presentation_alongside(dispatcher_p, &window_a)
            .expect("A installs alongside P with a real window mapping");
        let a_id = dispatcher_a.address.presentation_id;

        // Close A (non-primary) -- P survives, so the realm itself is
        // still perfectly live.
        close_presentation(dispatcher_p, a_id).expect("A closes through the real dispatch seam");

        // `dispatcher_a` is now stale: its own address is no longer
        // registered, even though `dispatcher_a.address.realm_id` still
        // names a live realm.
        let result = install_presentation_alongside(dispatcher_a, &window_c);
        assert!(
            matches!(result, Err(InstallPresentationError::StalePresentation)),
            "a dispatcher for a closed presentation must be refused as StalePresentation, even \
             though its realm survives, got {result:?}"
        );

        let count = Rc::new(Cell::new(None));
        let count_in_task = Rc::clone(&count);
        dispatch_platform_realm(
            dispatcher_p,
            RealmTask::Frame(Box::new(move |realm| {
                count_in_task.set(Some(realm.presentation_count()));
            })),
        )
        .expect("frame task dispatches");
        assert_eq!(
            count.get(),
            Some(1),
            "the rejected install must never have installed a third presentation -- only P \
             survives"
        );

        teardown_platform_realm();
    }
}

// ============================================================================
// Desktop frame-pacing gate (App.1 vsync pacing)
// ============================================================================
//
// Extracted as free functions — pure, no realm/window/GPU state — so the
// decisions each platform's frame callback makes each wake are unit
// testable without a live event loop. See the frame-pacing ADR for the
// full design: Fifo present blocks every PRESENTED frame at display
// cadence (the steady-state pacing); these functions cover what happens on
// the frames that path never blocks: a spurious wake with nothing to do or
// a backgrounded app (`wake_action`), and a frame that ran the pipeline but
// never reached `present()` (`no_present_fallback_pace`).

/// What a platform wake should do: run the full frame pipeline, pump only
/// the async driver, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WakeAction {
    /// Run the full frame pipeline — the pre-existing path, unchanged:
    /// frames are enabled and there is real work or a scheduled ticker.
    Render,
    /// Frames are disabled (`AppLifecycleState::Hidden`/`Paused`/
    /// `Detached`): poll only [`UpdateScheduler::drive_async_tasks`](flui_scheduler::UpdateScheduler::drive_async_tasks) — never
    /// begin/draw a frame, tick, run the pipeline, or present. Dirty work
    /// is left untouched; it accumulates until frames re-enable.
    PumpAsync,
    /// A spurious wake while frames are enabled: nothing dirty, no
    /// scheduled ticker. No render, no pump, no sleep.
    Skip,
}

/// Decides what a platform wake should do, given the scheduler's
/// [`UpdateScheduler::frames_enabled`](flui_scheduler::UpdateScheduler::frames_enabled) fact (ADR-0035) alongside the pre-existing
/// dirty/scheduled-ticker signals.
///
/// `frames_enabled == false` takes priority over everything else — even
/// with `dirty` work pending, a backgrounded app pumps only the async
/// driver; the dirty work is left alone (it accumulates untouched) rather
/// than running a full frame nobody can see. This is the ONLY thing that
/// keeps a spawned future progressing while the app is backgrounded: the
/// mid-frame `drive_async_tasks` poll inside `handle_begin_frame` never
/// runs in `PumpAsync` mode (no frame runs at all), so this explicit call
/// is the only pump.
///
/// `dirty` is true when there is real work (an inbox redraw request,
/// `needs_redraw`, or dirty pipeline nodes); `frame_scheduled` is true when
/// the global `UpdateScheduler` has a pending ticker callback (a running
/// `AnimationController` with no other dirty state).
fn wake_action(frames_enabled: bool, dirty: bool, frame_scheduled: bool) -> WakeAction {
    if !frames_enabled {
        return WakeAction::PumpAsync;
    }
    if dirty || frame_scheduled {
        WakeAction::Render
    } else {
        WakeAction::Skip
    }
}

/// Whether another frame will be requested regardless of this one's
/// outcome: `needs_redraw`, a scheduled ticker, or dirty
/// pipeline/build work left over from the frame that just ran.
///
/// This only feeds [`no_present_fallback_pace`]'s THROTTLE decision below —
/// it cannot itself wake anything. A `ControlFlow::Wait` loop only wakes on
/// an actual `wake_frame()`/platform `request_redraw()` call or external
/// input; a dropped/errored frame's retry wake comes from
/// `render_frame_entered`'s `retry_needed` path, not from this function.
///
/// The pending-work leg matters when a frame that left dirty pipeline/build
/// nodes behind is ALSO being re-invoked by some other wake source without
/// ever reaching `present()`: without this leg, such a frame would read
/// `keeps_gate_open == false`, skip the fallback sleep, and the loop could
/// spin at full CPU speed re-processing the same leftover work on every
/// rapid re-wake instead of being bounded like any other no-present,
/// gate-open frame.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn keeps_frame_gate_open(
    needs_redraw: bool,
    frame_scheduled: bool,
    has_pending_work: bool,
) -> bool {
    needs_redraw || frame_scheduled || has_pending_work
}

/// Coarse fallback pace for a frame that ran the pipeline but never reached
/// `present()`, applied only while a ticker keeps re-requesting a frame.
///
/// This throttles; it does not pace. An un-presented frame carries no vsync
/// signal (Fifo's blocking present never engaged), so this is a fixed CPU-time
/// bound, not frame-accurate cadence — good enough to keep a repeating
/// controller behind a minimized/occluded window (or a `SurfaceLost` retry
/// loop) from busy-spinning at CPU speed (observed pre-fix: ~30 000 fps).
///
/// Not `cfg`-gated to desktop-only: Android's `PumpAsync` arm (`run_android`)
/// reuses this same bound unconditionally — a self-re-arming task
/// `UpdateScheduler::finish_async_pump` lets keep waking the loop has no
/// vsync/present call to bound it there either, and that arm has no
/// `keeps_frame_gate_open`-style signal desktop's conditional
/// `no_present_fallback_pace` uses — so its throttle is unconditional
/// instead of gate-open-dependent. Web's `PumpAsync` arm does NOT use this
/// (see its call site's comment: the browser's own `requestAnimationFrame`
/// cadence already bounds it, and `wasm32-unknown-unknown` has no real
/// `std::thread::sleep`) — excluded via `cfg` so it isn't flagged unused
/// there.
#[cfg(not(target_arch = "wasm32"))]
const NO_PRESENT_FALLBACK_PACE: std::time::Duration = std::time::Duration::from_millis(16);

/// Decides whether [`NO_PRESENT_FALLBACK_PACE`] applies this frame.
///
/// `presented` is `false` when `render_frame_entered`'s scene never reached
/// `present()` — no damage, an occluded surface, or a lost surface.
/// `keeps_gate_open` is `true` when another frame will be requested
/// regardless (`UiRealm::needs_redraw` or the scheduler still has a
/// ticker scheduled). The fallback is needed only when both hold: no vsync
/// block happened AND something is about to wake this loop again anyway —
/// that combination is the only busy-spin risk left once the fixed
/// frame-budget sleep is gone. A presented frame needs no fallback (Fifo
/// already paced it); an un-presented frame with nothing re-requesting a
/// wake needs no fallback either (the loop just goes idle).
///
/// Occlusion semantics differ by platform: on Wayland, frame callbacks stop
/// while a window is hidden, so no redraws arrive and tickers freeze (this
/// fallback never fires); on Windows/X11, redraw requests keep arriving for a
/// minimized window and this fallback bounds them. Timeout-shaped animations
/// (e.g. the snack-bar auto-dismiss controller) do not progress while frozen —
/// a future platform Timer service is the correctness seam for those.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn no_present_fallback_pace(presented: bool, keeps_gate_open: bool) -> Option<std::time::Duration> {
    (!presented && keeps_gate_open).then_some(NO_PRESENT_FALLBACK_PACE)
}

/// App.1 vsync-pacing gate tests.
///
/// `run_desktop` itself opens a real window and GPU device, so it cannot
/// run headlessly; `wake_action` and `no_present_fallback_pace` were pulled
/// out specifically so the decisions the frame callback makes each wake are
/// covered here without one. Coverage map for the four invariants the
/// frame-pacing ADR calls out:
///
/// - **Wake coalescing** (N `wake_frame` calls -> one draw): a
///   PRE-EXISTING invariant, unchanged by this diff — pinned by
///   `ui_realm::tests::redraw_requests_coalesce_to_one_flag_and_one_wake`.
/// - **Idle = zero frames**: a PRE-EXISTING invariant (the dirty gate
///   itself predates this diff; only its migration onto `wake_action` is
///   new) — pinned by
///   `idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips`
///   below.
/// - **No-present fallback bound**: the actual delta the frame-pacing ADR
///   introduces — pinned by `no_present_fallback_bounds_repeating_no_present_wakes`.
/// - **Ticker keeps the gate open**: the fallback's AND condition — pinned
///   by `no_present_fallback_pace_requires_both_no_present_and_an_open_gate`
///   (this module) and, at the binding layer, by
///   `binding::tests::vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle`.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod desktop_pacing_tests {
    use std::time::{Duration, Instant};

    use super::{
        NO_PRESENT_FALLBACK_PACE, WakeAction, keeps_frame_gate_open, no_present_fallback_pace,
        wake_action,
    };

    #[test]
    fn idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips() {
        assert_eq!(
            wake_action(true, false, false),
            WakeAction::Skip,
            "a spurious wake with frames enabled, nothing dirty, and no scheduled ticker must \
             render zero frames"
        );
    }

    #[test]
    fn dirty_work_or_a_scheduled_ticker_alone_renders_a_frame() {
        assert_eq!(
            wake_action(true, true, false),
            WakeAction::Render,
            "dirty work alone renders"
        );
        assert_eq!(
            wake_action(true, false, true),
            WakeAction::Render,
            "a scheduled ticker alone renders (keeps animations alive with no other dirty state)"
        );
        assert_eq!(wake_action(true, true, true), WakeAction::Render);
    }

    #[test]
    fn frames_disabled_always_pumps_async_regardless_of_dirty_or_scheduled() {
        // The load-bearing case: a backgrounded app must never render, even
        // with real dirty work or a scheduled ticker — dirty work
        // accumulates untouched until frames re-enable.
        assert_eq!(wake_action(false, false, false), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, true, false), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, false, true), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, true, true), WakeAction::PumpAsync);
    }

    /// A spawned future must keep progressing through `PumpAsync`'s
    /// `UpdateScheduler::drive_async_tasks` call while frames are disabled, with
    /// no frame ever advancing — and a `Resumed` transition afterward must
    /// produce exactly one frame.
    ///
    /// Standalone `UpdateScheduler::new()`, not the process singleton: this test
    /// mirrors what `run_desktop`'s frame callback does on a `PumpAsync`
    /// wake, without needing a live window/event loop.
    ///
    /// If reverted: gate the pump too (only call `drive_async_tasks` when
    /// `wake_action` returns `Render` — a mistaken "no work while
    /// backgrounded" fix) and this fails: the future never completes (RUN
    /// IT — see the test module doc for how this is verified).
    #[test]
    fn frames_disabled_pump_async_keeps_futures_running_without_advancing_frames() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};

        use flui_scheduler::{AppLifecycleState, UpdateScheduler};

        let scheduler = UpdateScheduler::new();
        let polls = std::sync::Arc::new(AtomicUsize::new(0));
        let completed = std::sync::Arc::new(AtomicBool::new(false));
        let polls_for_task = std::sync::Arc::clone(&polls);
        let completed_for_task = std::sync::Arc::clone(&completed);
        // Needs two polls to complete, so the loop below observes both the
        // Pending and the Ready poll — proving `drive_async_tasks` is what
        // actually advances it, not a single incidental call.
        let _token = scheduler.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            let n = polls_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if n < 2 {
                // Re-arm itself for the next `drive_async_tasks` call —
                // without this, `poll_ready` would only ever poll it once
                // (nothing else wakes it), and the second loop iteration
                // below would silently poll zero tasks.
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            } else {
                completed_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
                std::task::Poll::Ready(())
            }
        })));

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert!(!scheduler.frames_enabled());

        let frame_count_before = scheduler.frame_count();
        for _ in 0..2 {
            assert_eq!(
                wake_action(
                    scheduler.frames_enabled(),
                    true,
                    scheduler.is_frame_scheduled()
                ),
                WakeAction::PumpAsync,
                "frames disabled must always pump, even with dirty work"
            );
            scheduler.drive_async_tasks();
        }

        assert!(
            completed.load(std::sync::atomic::Ordering::SeqCst),
            "the future must complete via PumpAsync's drive_async_tasks calls alone"
        );
        assert_eq!(
            scheduler.frame_count(),
            frame_count_before,
            "no frame may run while the app is backgrounded"
        );

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(
            wake_action(
                scheduler.frames_enabled(),
                true,
                scheduler.is_frame_scheduled()
            ),
            WakeAction::Render
        );
        let now = web_time::Instant::now();
        scheduler.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {});
        assert_eq!(
            scheduler.frame_count(),
            frame_count_before + 1,
            "resuming must produce exactly one frame"
        );
    }

    #[test]
    fn pending_work_alone_keeps_the_gate_open() {
        assert!(
            keeps_frame_gate_open(false, false, true),
            "a frame that left dirty pipeline/build nodes behind must keep the fallback-pace \
             gate open (so the busy-spin throttle still applies on a rapid re-wake) even with \
             no `needs_redraw` and no scheduled ticker"
        );
    }

    #[test]
    fn needs_redraw_or_scheduled_ticker_alone_keeps_the_gate_open() {
        assert!(keeps_frame_gate_open(true, false, false));
        assert!(keeps_frame_gate_open(false, true, false));
    }

    #[test]
    fn no_signal_at_all_closes_the_gate() {
        assert!(
            !keeps_frame_gate_open(false, false, false),
            "with no redraw request, no scheduled ticker, and no pending work, the gate \
             must close so the loop can go idle"
        );
    }

    #[test]
    fn pending_work_drives_the_no_present_fallback_pace_like_any_other_open_gate() {
        // A frame that never presents (surface lost / no damage) but left dirty
        // pipeline work behind must still get the busy-spin-bounding fallback pace —
        // exactly as if `needs_redraw` or a ticker had kept the gate open.
        let keeps_gate_open = keeps_frame_gate_open(false, false, true);
        assert_eq!(
            no_present_fallback_pace(false, keeps_gate_open),
            Some(NO_PRESENT_FALLBACK_PACE)
        );
    }

    #[test]
    fn no_present_fallback_pace_requires_both_no_present_and_an_open_gate() {
        assert_eq!(
            no_present_fallback_pace(true, true),
            None,
            "a presented frame needs no fallback — Fifo present already paced it"
        );
        assert_eq!(
            no_present_fallback_pace(true, false),
            None,
            "a presented frame with a closing gate needs no fallback either"
        );
        assert_eq!(
            no_present_fallback_pace(false, false),
            None,
            "an un-presented frame with nothing re-requesting a wake needs no fallback \
             — the loop simply goes idle, no busy-spin risk"
        );
        assert_eq!(
            no_present_fallback_pace(false, true),
            Some(NO_PRESENT_FALLBACK_PACE),
            "the only busy-spin risk: no present AND a ticker keeps re-requesting a frame"
        );
    }

    /// Mutation-run target for the no-present fallback bound: simulates the shape of
    /// `run_desktop`'s frame callback for a window that never presents
    /// (e.g. minimized/occluded) while a repeating ticker keeps
    /// re-requesting a frame every wake — the exact scenario that used to
    /// busy-spin at CPU speed (observed pre-fix: ~30 000 fps) once the
    /// fixed frame-budget sleep this diff removes was the only thing
    /// bounding it.
    ///
    /// This cannot drive the real winit closure (it requires a live event
    /// loop), so it exercises the same predicate + `thread::sleep` pairing
    /// `run_desktop` calls, in a tight loop bounded by wall-clock time.
    /// Deleting the `sleep` (or the `if let Some` guard around it) turns
    /// this from ~5 iterations in the test window into whatever the CPU
    /// can spin through in that time — comfortably over the assertion's
    /// generous ceiling.
    #[test]
    fn no_present_fallback_bounds_repeating_no_present_wakes() {
        let window = Duration::from_millis(80);
        let deadline = Instant::now() + window;
        let mut iterations = 0u32;

        while Instant::now() < deadline {
            iterations += 1;
            let presented = false; // simulated: no damage / occluded / surface lost
            let keeps_gate_open = true; // simulated: a repeating AnimationController
            if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                std::thread::sleep(pace);
            }
        }

        assert!(
            iterations < 50,
            "no-present fallback failed to bound the loop: {iterations} iterations in \
             {window:?} (expected roughly window / NO_PRESENT_FALLBACK_PACE, generously \
             capped) — a busy-spin without it would rack up orders of magnitude more",
        );
    }

    /// Reviewer probe (issue #556): does a running controller with no other
    /// tree-visible effect survive `on_request_frame`'s OWN dirty gate
    /// across many platform callbacks, or does it stall after its anchor
    /// frame?
    ///
    /// The question this settles: `draw_frame_entered`'s vsync-continuation
    /// loop calls `wake_frame()` — which sets `needs_redraw = true` — BEFORE
    /// the segment that renders the frame; `render_frame_entered`'s own
    /// tail then calls `mark_rendered()` — which sets `needs_redraw =
    /// false` — once that render succeeds with no retry needed. Both run
    /// inside the SAME production callback, so `needs_redraw` reads `false`
    /// again by the time this callback returns. The open question was
    /// whether `wake_action`'s dirty check on the NEXT callback (computed
    /// from `needs_redraw()`/`has_pending_work()`, sampled BEFORE
    /// `draw_frame_entered` ever runs, i.e. before the controller gets a
    /// chance to tick again and re-mark demand) would read `Skip` and never
    /// call `draw_frame_entered` at all — silently stalling the controller
    /// after one frame, forever, in production.
    ///
    /// This is deliberately the exact sequence `bootstrap_desktop`'s
    /// `on_request_frame` closure runs, minus the owner-inbox drain (this
    /// probe has nothing in the inbox) and the actual GPU/platform calls
    /// (`record_compositor_tick` -> the `dirty`/`wake_action` gate ->
    /// `render_frame_entered` -> repeat), not `draw_frame_entered` called
    /// directly the way the segment-gate unit tests do — a direct call
    /// bypasses the exact gate this probe exists to settle.
    #[test]
    fn a_running_controller_with_no_other_dirty_state_keeps_producing_across_the_real_wake_action_gate()
     {
        use flui_animation::{Animation, AnimationController, AnimationStatus};
        use flui_engine::{EngineError, RasterBackend};
        use flui_layer::Scene;

        struct AlwaysPresentsProbeBackend;

        impl RasterBackend for AlwaysPresentsProbeBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                Ok(true)
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
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

        let realm = super::super::ui_realm::UiRealm::for_test();
        let controller = AnimationController::new(
            Duration::from_millis(100),
            &flui_scheduler::UpdateScheduler::new(),
        );
        realm.vsync().register(controller.clone());
        controller.forward().expect("fresh controller forwards");
        // The anchor: starting a controller does not itself wake anything
        // (`AnimationController::forward`/`Vsync::register` are pure
        // state mutations with no coupling to a realm's wake capability —
        // confirmed by reading both, neither calls anything wake-shaped).
        // In production SOMETHING external always establishes the first
        // wake before a freshly-started controller's own continuation
        // logic can keep the loop going (the gesture/build code path that
        // called `.forward()` in the first place almost always also
        // dirties the tree or explicitly redraws) — simulated here as one
        // `request_redraw()` call, the same flag `wake_action`'s dirty
        // check reads, without needing a real platform window to poke.
        realm.request_redraw();

        let mut backend = AlwaysPresentsProbeBackend;
        let mut now = Instant::now();
        let mut render_calls = 0u32;
        let mut skip_calls = 0u32;

        // 40 simulated platform callbacks at ~16ms apart -- comfortably
        // past the controller's 100ms duration, so a genuine stall shows
        // up as `status() != Completed` at the end, not just a slow test.
        for callback in 0..40u32 {
            now += Duration::from_millis(16);
            realm.set_now_secs_for_test(f64::from(callback) * 0.016);

            // Exactly what `bootstrap_desktop`'s closure does, in order:
            // record the compositor tick first (pacing feedback only, marks
            // no demand -- see `FrameClock::record_compositor_tick`'s own
            // doc), THEN read the dirty gate `wake_action` decides from.
            realm.record_compositor_tick(now);
            let dirty = realm.needs_redraw() || realm.has_pending_work();
            match wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled(),
            ) {
                WakeAction::Skip => {
                    skip_calls += 1;
                    continue;
                }
                WakeAction::PumpAsync => panic!(
                    "frames must stay enabled for this probe -- a PumpAsync wake would mean \
                     something else broke lifecycle state, not the property under test"
                ),
                WakeAction::Render => {}
            }
            let _presented = realm.render_frame_entered(&mut backend);
            render_calls += 1;
        }

        assert_eq!(
            controller.status(),
            AnimationStatus::Completed,
            "a running controller with no other dirty state must reach completion across \
             real platform callbacks gated by wake_action -- {render_calls} renders, \
             {skip_calls} skips out of 40 callbacks, final value={}",
            controller.value()
        );
        assert!(
            render_calls > 1,
            "sanity: completion in exactly one render would not distinguish this from a \
             single-anchor-frame stall followed by the loop simply running out of callbacks"
        );

        controller.dispose();
    }

    /// The idle/instant-response headline (adaptive on-demand pacing),
    /// driven through the SAME real dirty-gate every other production wake
    /// goes through -- `wake_action`, `keeps_frame_gate_open`, and
    /// `no_present_fallback_pace` -- not a bare fixed-interval poll loop.
    ///
    /// This distinction is load-bearing, not stylistic: `has_pending_work`
    /// includes `gestures().has_pending_deadlines()`, so a pending
    /// gesture-arena deadline keeps `dirty == true` for the ENTIRE time it
    /// is armed. An earlier version of this test called
    /// `drive_frame_with_lane` on a fixed 5ms interval regardless of this
    /// gate, which made "zero produces" true but hid a real cost: driven
    /// through the real gate, ANY wake while the deadline is
    /// armed-but-not-due reads `dirty == true` -> `WakeAction::Render` ->
    /// a full (unpainting) pump -> `presented == false`, and with the gate
    /// still open afterward, `no_present_fallback_pace` would sleep on the
    /// calling thread. What keeps this genuinely idle is that NOTHING
    /// wakes the loop until the deadline's own instant (`UiRealm::next_wake`,
    /// standing in for the real winit `about_to_wait`/`new_events`
    /// actuator this crate cannot drive with a live event loop under its
    /// own test harness) -- so there is exactly ONE real callback over the
    /// whole window, not many, and the fallback sleep never engages.
    #[test]
    fn idle_presentation_over_a_real_wall_clock_window_produces_zero_then_responds_instantly() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use flui_engine::{EngineError, RasterBackend};
        use flui_interaction::{
            GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
        };
        use flui_layer::Scene;

        struct CountingProbeBackend {
            render_scene_calls: u32,
        }

        impl RasterBackend for CountingProbeBackend {
            fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
                self.render_scene_calls += 1;
                Ok(true)
            }
            fn resize(&mut self, _width: u32, _height: u32) {}
            fn is_device_lost(&self) -> bool {
                false
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

        let realm = super::super::ui_realm::UiRealm::for_test();
        realm
            .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
            .expect("attach succeeds");
        let mut backend = CountingProbeBackend {
            render_scene_calls: 0,
        };

        // Settle the attach's own first paint, through the real gate --
        // so the "instant response" check at the end has real content to
        // actually submit, not a vacuous zero-widget tree.
        let now = Instant::now();
        realm.record_compositor_tick(now);
        let dirty = realm.needs_redraw() || realm.has_pending_work();
        assert_eq!(
            wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled()
            ),
            WakeAction::Render,
            "precondition: the attach's own pending build must render"
        );
        realm.scheduler().drive_frame_with_lane(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {
                let _ = realm.render_frame_entered(&mut backend);
            },
            realm.local_post_frame_lane(),
        );

        // A live async task, polled through the scheduler's real mid-frame
        // slot whenever a pump actually runs -- it never touches
        // demand/dirty state.
        let async_polls = Arc::new(AtomicUsize::new(0));
        let async_polls_for_task = Arc::clone(&async_polls);
        let _token = realm.scheduler().spawn_local(Box::pin(async move {
            async_polls_for_task.fetch_add(1, Ordering::Release);
        }));

        // A real, in-flight gesture-arena deadline, due 100ms from now.
        let arena = realm.gestures().arena().clone();
        let long_press_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_for_callback = Arc::clone(&long_press_fired);
        let recognizer = LongPressGestureRecognizer::with_settings(
            arena,
            GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(100)),
        )
        .with_on_long_press(move || fired_for_callback.store(true, Ordering::SeqCst));
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        recognizer.add_pointer(
            pointer,
            flui_types::Offset::new(
                flui_types::geometry::px(10.0),
                flui_types::geometry::px(10.0),
            ),
        );

        let produced_before = realm.primary_produced_count_for_test();
        let submits_before = backend.render_scene_calls;

        // Drive the loop waking ONLY at `next_wake`'s own computed instant
        // -- no artificial fixed-interval polling.
        let window_end = Instant::now() + Duration::from_millis(300);
        let mut render_calls = 0u32;
        let mut skip_calls = 0u32;
        // `next_wake() == None` means production would genuinely fall back
        // to `ControlFlow::Wait` here (nothing left to wait for) -- once the
        // one callback below resolves the deadline, that is the expected,
        // successful end of the simulated window, not a bug, so the loop
        // condition itself ends it.
        while let Some(next_wake) = realm.next_wake() {
            if next_wake >= window_end {
                break;
            }
            let wait = next_wake.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            let now = Instant::now();
            realm.record_compositor_tick(now);
            let dirty = realm.needs_redraw() || realm.has_pending_work();
            match wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled(),
            ) {
                WakeAction::Skip => {
                    skip_calls += 1;
                    continue;
                }
                WakeAction::PumpAsync => panic!(
                    "frames must stay enabled for this probe -- a PumpAsync wake would mean \
                     something else broke lifecycle state, not the property under test"
                ),
                WakeAction::Render => {}
            }
            // Wrapped in `drive_frame_with_lane`, exactly like
            // `bootstrap_desktop`'s `on_request_frame` closure -- this is
            // the ONLY call site that actually polls the async driver's
            // mid-frame slot; `render_frame_entered` alone does not.
            realm.scheduler().drive_frame_with_lane(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {
                    let presented = realm.render_frame_entered(&mut backend);
                    let keeps_gate_open = keeps_frame_gate_open(
                        realm.needs_redraw(),
                        realm.scheduler().is_frame_scheduled(),
                        realm.has_pending_work(),
                    );
                    if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                        std::thread::sleep(pace);
                    }
                },
                realm.local_post_frame_lane(),
            );
            render_calls += 1;
        }

        assert_eq!(
            realm.primary_produced_count_for_test(),
            produced_before,
            "an idle presentation, driven through the real wake_action gate, must produce \
             exactly zero additional frames over the wall-clock window -- {render_calls} \
             render callbacks, {skip_calls} skips"
        );
        assert_eq!(
            backend.render_scene_calls, submits_before,
            "and submit exactly zero additional frames to the raster backend"
        );
        assert_eq!(
            render_calls, 1,
            "exactly ONE real callback over the whole idle window -- the deadline's own wake, \
             nothing polled it more often than that (render_calls={render_calls}, \
             skip_calls={skip_calls})"
        );
        assert_eq!(
            async_polls.load(Ordering::Acquire),
            1,
            "the async driver's mid-frame poll must have genuinely run the spawned task, at \
             the one real callback that occurred"
        );
        assert!(
            long_press_fired.load(Ordering::SeqCst),
            "the deadline must have resolved by the end of the window"
        );

        // Instant response: dirty the tree now and drive exactly one more
        // real callback through the same gate -- must produce.
        realm.request_redraw();
        let dirty = realm.needs_redraw() || realm.has_pending_work();
        assert_eq!(
            wake_action(
                realm.scheduler().frames_enabled(),
                dirty,
                realm.scheduler().is_frame_scheduled()
            ),
            WakeAction::Render,
            "precondition: the fresh redraw request must render"
        );
        let _ = realm.render_frame_entered(&mut backend);

        assert_eq!(
            realm.primary_produced_count_for_test(),
            produced_before + 1,
            "the first dirty mark after the idle window must produce within the very next \
             real callback"
        );
        assert_eq!(
            backend.render_scene_calls, 1,
            "and that callback must actually reach the raster backend"
        );
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use super::hot_reload::{RebuildHookGuard, WorkerReload};

    tracing::info!("Starting desktop platform via flui-platform");

    // Development reload, if this build has it: with the `hot-reload` feature
    // off this value is inert and `flui-hot-reload` is not in the graph.
    let worker_reload = WorkerReload::from_config(&config);

    // Platform init is an environment failure (missing display server, unsupported
    // OS, driver problem), not a `BUG:` invariant — no `bootstrap_error_slot` exists
    // yet to route this through (that cell, and the `platform` it needs for
    // `quit()`, only exist once `on_ready` is running), so this is the one desktop
    // failure this function still surfaces via `panic!` directly rather than the
    // deferred-teardown path below. It still gets a full error log and the same
    // "desktop bootstrap failed" wording as that deferred path, instead of a bare
    // `.expect()`'s terse, context-free message.
    let platform = match flui_platform::current_platform() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "Failed to initialize platform");
            panic!("desktop bootstrap failed: platform initialization error: {error:?}");
        }
    };

    // `rebuild_registration`'s `Drop` detaches the hot-reload hook and must
    // stay alive until the event loop exits — but it (like the window and
    // every callback below) can only be created from inside `on_ready`, so
    // it is threaded back out through this cell instead of a plain local.
    let rebuild_registration: Rc<RefCell<Option<RebuildHookGuard>>> = Rc::new(RefCell::new(None));
    let rebuild_registration_slot = Rc::clone(&rebuild_registration);

    /// The actual desktop bootstrap: opens the window, initializes the GPU
    /// renderer, mounts the widget tree, and wires every platform/window
    /// callback. Runs exactly once, synchronously, inside `on_ready` (see
    /// `Platform::run`'s doc) — never before, since the winit backend can
    /// only create a window from inside a running event loop
    /// (`ActiveEventLoop` is unreachable beforehand, and `open_window` fails
    /// fast rather than deadlock if called too early).
    ///
    /// Returns `Err` on any bootstrap failure (GPU init, `UiRealm`
    /// construction, root widget attach); `on_ready` itself is now fallible,
    /// so this propagates straight out of `Platform::run` instead of
    /// threading the failure out through a
    /// `Rc<RefCell<Option<anyhow::Error>>>` side channel — that pattern is
    /// now redundant here and has been removed. Every backend stops
    /// entering (or promptly exits) its loop on this `Err`, so there is no
    /// need to call `owner.quit()` explicitly on any of the error paths
    /// below.
    ///
    /// Pulled out of the `on_ready` closure into a named fn so rustfmt
    /// actually formats it — rustfmt does not reliably reformat very large
    /// closure literals passed as call arguments.
    fn bootstrap_desktop<V>(
        root: V,
        config: AppConfig,
        worker_reload: WorkerReload,
        rebuild_registration_slot: Rc<RefCell<Option<RebuildHookGuard>>>,
    ) -> anyhow::Result<()>
    where
        V: View + StatelessView + Clone + 'static,
    {
        tracing::info!("Platform ready");

        // No `owner: OwnerPlatform` parameter: the caller already installed
        // it in the loop-scoped host (ADR-0039 §6) before calling this
        // function, so every owner-thread touch below re-crosses the fenced
        // `with_owner_platform` accessor instead of holding a private copy
        // (`OwnerPlatform` isn't `Clone` by design — there is exactly one
        // instance, and the TLS host is its one sanctioned home for the
        // rest of the loop's life).
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_desktop runs only after install_owner_platform")
        }

        // 0. Wire the platform clipboard (ADR-0034) before anything else can
        // observe `AppRuntime::clipboard()`.
        let clipboard = owner_platform_installed(|owner| owner.shared().clipboard());
        APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));

        // 0b. Wire the exit-policy hook (issue #555's native-lifecycle
        // wiring): the winit backend's `CloseRequested` handling consults
        // this instead of deciding from its own native window count alone,
        // so a queued "open the main window" install (this same thread's
        // `AppRuntime`, invisible to the platform layer) can veto an exit
        // the backend would otherwise take unconditionally.
        install_exit_policy_hook(config.exit_policy);

        // 0c. Wire the wall-clock-wake hook: the winit
        // backend's `about_to_wait` consults this every idle iteration
        // instead of blocking forever, so a pending gesture-arena deadline
        // (a long-press hold, a double-tap give-up) still wakes the loop
        // at the right instant even while nothing else is dirty and no
        // animation is running.
        install_wake_deadline_hook();

        // 1. Open window now that the event loop is running. Window creation is
        // an environment failure (display server hiccup, resource exhaustion),
        // not a `BUG:` invariant, and — unlike platform init above — this DOES
        // run inside `on_ready` with a live owner capability, so a failure here
        // gets the same `Err`-propagates-out-of-`run` handling as the
        // GPU/realm/attach failures below instead of an immediate bare
        // `.expect()` panic mid-`on_ready`. `Ready` is guaranteed here
        // (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let window = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Window creation failed");
                return Err(anyhow::Error::from(error).context("Window creation failed"));
            }
        };

        // 2. Create GPU renderer directly (no DesktopEmbedder)
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(Renderer::new(window.as_ref()));
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return Err(anyhow::anyhow!(e).context("GPU init failed"));
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget at the LOGICAL size; the framework lays out
        // in logical pixels and the paint root's DPR transform maps to the
        // physical surface. `UiRealm::new` applies the DPR to the freshly
        // built pipeline before returning, so the RenderView configuration
        // and the first frame agree on the scale from construction.
        let scale_factor = window.scale_factor() as f32;
        let wake = runtime_wake_callback();
        let ui_realm = match super::ui_realm::UiRealm::new(
            Arc::clone(&wake),
            Arc::clone(&window),
            scale_factor,
            runtime_needs_redraw_handle(),
        ) {
            Ok(realm) => realm,
            Err(e) => {
                tracing::error!(error = %e, "UiRealm construction failed");
                return Err(anyhow::anyhow!(e).context("UiRealm construction failed"));
            }
        };

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_realm.set_performance_overlay(config.show_performance_overlay);

        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            realm.attach_root_widget_with_size(&root, logical.width.0, logical.height.0)
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
        }

        // 3b. Wire the wake chain (E0a).
        //
        // `on_need_frame` fires whenever `handle_build_scheduled` determines a new
        // frame is required (e.g. after setState).  The closure calls `wake`
        // which sets `needs_redraw` atomically AND calls `PlatformWindow::
        // request_redraw()` so the winit event loop wakes from idle.
        //
        // Deadlock analysis:
        // * `wake` acquires only the loop-scoped redraw-window leaf Mutex.
        // * The closure is called from `handle_build_scheduled`, which holds no
        //   `inner`/`widgets` lock (see `WidgetsBinding::handle_build_scheduled`
        //   doc).
        // * `on_need_frame` itself is a separate `RwLock` on `WidgetsBinding`,
        //   never held across any `inner` critical section.
        // Therefore: no lock ordering conflict.
        {
            let widgets = ui_realm.widgets();
            let wake = Arc::clone(&wake);
            widgets.set_on_need_frame(move || wake());
        }

        // Wire `on_build_scheduled` on the BuildOwner so a dirty-element
        // registration (e.g. from setState inside an element build) wakes the
        // platform loop. The callback fires from inside `schedule_build_for`,
        // which runs during a build while the realm's `widgets` write lock is
        // held — so it must NOT re-lock `widgets`. It calls `wake`
        // directly (the same effect as the `on_need_frame` callback above),
        // which touches only the loop-scoped redraw-window leaf lock. The
        // callback must not re-enter widget state while `BuildOwner` is
        // scheduling; realm entry is reserved for the outer event/frame
        // dispatch boundary.
        {
            let widgets = ui_realm.widgets();
            widgets.with_build_owner_mut(|build_owner| {
                let wake = Arc::clone(&wake);
                build_owner.set_on_build_scheduled(move || wake());
            });
        }

        // 3c. Construct the per-window owner and its bounded command inbox.
        // The wake is the existing chain: `wake_frame` sets
        // `needs_redraw` and queues a `RedrawRequested`, so a command sent to an
        // idle loop produces the frame whose drain observes it.
        //
        tracing::info!(
            { flui_foundation::diagnostics::PRESENTATION_ID } = ui_realm.presentation_id().as_u64(),
            inbox_capacity = ui_realm.command_sender().capacity(),
            "UiRealm constructed"
        );
        let hot_reload_sender = ui_realm.command_sender();
        let realm_dispatch = install_platform_realm(ui_realm, &window);
        *rebuild_registration_slot.borrow_mut() =
            Some(worker_reload.register_rebuild_hook(hot_reload_sender));

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // Install the registration-lifetime surface applier alongside the
        // realm (cleared together at teardown): a `Resized` event takes it
        // out of the TLS slot, calls it, and restores it (see
        // `PlatformToUi::run`'s `Resized` arm) rather than capturing the
        // renderer inside the event payload itself.
        {
            let renderer_resize = Arc::clone(&renderer);
            install_surface_applier(
                realm_dispatch.address.realm_id,
                move |size, scale_factor| {
                    let w = (size.width.0 * scale_factor) as u32;
                    let h = (size.height.0 * scale_factor) as u32;
                    renderer_resize.lock().resize(w, h);
                },
            );
        }

        // 5. Register input callback -> entered realm input dispatch
        window.on_input(Box::new(move |input: PlatformInput| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Input(input)),
            );
            DispatchEventResult::resolved(false, true)
        }));

        // 6. Register frame callback -> scheduler + UiRealm::render_frame_entered()
        let renderer_frame = Arc::clone(&renderer);
        let worker_reload_frame = worker_reload.clone();
        window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let worker_reload_frame = worker_reload_frame.clone();
        let _ = dispatch_platform_realm(realm_dispatch, RealmTask::Frame(Box::new(move |realm| {
            worker_reload_frame.poll_and_apply(realm);

            let scheduler = realm.scheduler();

        // Every fire of this callback is a genuine platform-delivered
        // frame-request signal on this backend (`WinitWindowEvent::
        // RedrawRequested` -> `dispatch_request_frame` -> here; see
        // `docs/adr/ADR-0044-driver-loop-hybrid.md`'s per-platform table for which backends
        // pace this via the compositor vs. deliver it immediately).
        // Recorded unconditionally, before the dirty/wake_action gate below
        // decides whether anything actually runs this pump: pacing
        // feedback is about observing the PLATFORM's own delivery timing,
        // independent of whether this particular delivery ends up idle.
        // `now` is read ONCE here and reused ~60 lines below at this
        // closure's `drive_frame_with_lane(now, ...)` call, past the
        // `wake_action` match below (that later call site's own comment
        // points back to this one) — this pump's pacing-feedback sample
        // and its own frame-drive instant must agree, the same
        // single-`now`-per-pump discipline every other call site in this
        // closure already follows.
            let now = web_time::Instant::now();
            realm.record_compositor_tick(now);

        // Owner-inbox drain: commands and worker results
        // commit HERE, at the frame boundary while the scheduler phase is
        // Idle — never inside the frame transaction below. Runs before the
        // dirty gate so a command-driven redraw request is observed by the
        // very frame its wake produced.
        //
        // The runtime is TAKEN out of the slot for the drain (and restored
        // after) so drained user closures never run under the RefCell
        // borrow: a command that re-enters this frame callback through a
        // nested platform pump then finds an empty slot and skips the
        // drain, instead of panicking the borrow.
            let inbox_redraw = drain_owner_inbox(realm);

            let dirty =
                inbox_redraw || realm.needs_redraw() || realm.has_pending_work();
            match wake_action(scheduler.frames_enabled(), dirty, scheduler.is_frame_scheduled()) {
                WakeAction::Skip => return,
                WakeAction::PumpAsync => {
                    // Frames disabled (Hidden/Paused/Detached): the mid-frame
                    // `drive_async_tasks` poll inside `handle_begin_frame`
                    // never runs because no frame runs at all — this
                    // explicit call is the ONLY thing keeping a spawned
                    // future progressing while backgrounded. No begin/draw
                    // frame, no tickers, no pipeline, no present.
                    //
                    // `finish_async_pump` MUST run first, not after: nothing
                    // else ever clears the scheduler's `frame_scheduled`
                    // latch on this path (only `handle_begin_frame` does,
                    // and it never runs here), so without this call a LATER,
                    // independent wake (a network response's `Waker::wake`,
                    // arriving after this pump cycle returns) would find the
                    // latch already set, never re-fire `on_frame_scheduled`,
                    // and never wake this loop again — see
                    // `UpdateScheduler::finish_async_pump`'s doc for the full
                    // starvation hazard and why the ordering matters.
                    scheduler.finish_async_pump();
                    scheduler.drive_async_tasks();
                    // Reuse the existing no-present throttle: a backgrounded
                    // wake with dirty/pending work re-requesting another
                    // wake every loop tick has the identical busy-spin risk
                    // an un-presented frame with an open gate has, and
                    // nothing else paces it while frames are disabled.
                    let keeps_gate_open = keeps_frame_gate_open(
                        realm.needs_redraw(),
                        scheduler.is_frame_scheduled(),
                        realm.has_pending_work(),
                    );
                    if let Some(pace) = no_present_fallback_pace(false, keeps_gate_open) {
                        std::thread::sleep(pace);
                    }
                    return;
                }
                WakeAction::Render => {}
            }

        // The `now` used below is the SAME instant bound near the top of
        // this closure and already recorded into `record_compositor_tick`
        // there (see the comment at that earlier `let now` binding) --
        // reused here, not re-read fresh.
        // UpdateScheduler callbacks (animations). NOTE: the global `UpdateScheduler` is driven
        // off this per-frame `Instant::now()`, while the tree-bound `Vsync`
        // (`UiRealm::draw_frame`) ticks off the realm's own `start` origin —
        // two separate clocks ON PURPOSE: the controller sets are disjoint (implicit
        // animations register with `Vsync`; plain controllers carry a private
        // `UpdateScheduler` ticker, never the global one), so the origins never need to
        // agree and no controller is advanced twice.
        // The ONE shared frame ordering — begin (transient +
        // microtasks + the single async-driver poll) -> persistent callbacks ->
        // the pipeline below -> post-frame callbacks -> Idle. `HeadlessBinding`
        // drives the same helper on its binding-local scheduler.
            let presented = scheduler.drive_frame_with_lane(now, flui_scheduler::IdleDeadline::far_future(now), || {
            // Render frame via the realm
            let mut r = renderer_frame.lock();
                let did_present = realm.render_frame_entered(&mut *r);

            // GPU device-loss recovery: if the device was lost during this frame
            // (detected by the wgpu callback that fired between render_frame calls),
            // attempt a synchronous rebuild on the runner thread. `pollster` is
            // already a dep and safe to use here — the desktop runner owns this
            // synchronous callback, not an async executor.
            if r.is_device_lost() {
                match pollster::block_on(r.recover()) {
                    Ok(()) => {
                        tracing::warn!("GPU device lost — recovered successfully");
                        // `wake_frame` (not `request_redraw`) so an idle winit loop
                        // actually queues a `RedrawRequested`: device loss is
                        // detected on a quiescent loop, where only flipping the
                        // `needs_redraw` flag would leave the recovered renderer
                        // idle until the next external input/resize.
                        realm.wake_frame();
                    }
                    Err(e) => {
                        // Driver may still be resetting. Log and let the next frame
                        // retry; the device-lost flag remains set so recover() will
                        // be tried again.
                        tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                    }
                }
            }
                did_present
            }, realm.local_post_frame_lane());

        // No-present fallback throttle. Fifo present (the default, see
        // `select_present_mode`) blocks every PRESENTED frame at display
        // cadence — that IS the steady-state pacing, which is why the fixed
        // frame-budget sleep this replaced is gone. A frame that never
        // reaches `present()` (no damage, occluded surface, surface lost)
        // gets none of that blocking, so if nothing else is going to wake
        // this loop, an unpaced wake is harmless: the loop falls back to
        // `ControlFlow::Wait` and blocks on the next real event. The
        // busy-spin this guards against (observed: ~30 000 fps) only
        // happens when a ticker/animation keeps re-requesting a frame every
        // wake with nothing pacing it — `no_present_fallback_pace` fires
        // only in exactly that combination.
            let keeps_gate_open = keeps_frame_gate_open(
                realm.needs_redraw(),
                scheduler.is_frame_scheduled(),
                realm.has_pending_work(),
            );
            if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                // This runs on the platform event-loop thread, so the sleep
                // blocks input dispatch for its duration — acceptable here
                // because this path only fires for an occluded/undamaged
                // window with a ticker still running, not an interactive one.
                std::thread::sleep(pace);
            }
        })));
    }));

        // 7. Register resize callback -> typed Resized event; the applier
        // installed above (not this closure) actually touches the renderer.
        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
            );
        }));

        // 8. Lifecycle callbacks
        //
        // Detached is dispatched through the realm because shutdown must
        // cancel any pointer sequence whose platform Up/Cancel will never
        // arrive before lifecycle observers run.

        // Platform quit -> Detached (frames disabled, listeners notified).
        // Fallback path, not the primary one any more: the ORDINARY
        // "close the last window -> exit" sequence now delivers Detached
        // earlier, from `close_this_window`'s own `RealmTask::
        // ClosePresentation` handling (the sole-presentation branch), before
        // this realm is even uninstalled -- so by the time `on_quit` fires
        // moments later, this dispatch routinely finds the realm already
        // gone. This callback stays registered for the case that path does
        // NOT cover: a quit requested with no preceding window close at all
        // (an OS-level quit signal, e.g. macOS Cmd+Q, or an embedder calling
        // `owner.quit()` directly).
        owner_platform_installed(|owner| {
            owner.shared().on_quit(Box::new(move || {
                tracing::info!("Platform quit");
                debug_assert_eq!(
                    std::thread::current().id(),
                    realm_dispatch.owner_thread,
                    "platform on_quit must fire on the realm's owner thread"
                );
                if let Err(error) = dispatch_platform_realm(
                    realm_dispatch,
                    RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Detached)),
                ) {
                    // Debug-only, not warn: the ordinary window-close-then-
                    // quit sequence above ALWAYS reaches this dispatch after
                    // the realm is already gone (Detached already delivered,
                    // the realm already uninstalled) -- an error here is the
                    // routine case, not a signal something went wrong.
                    tracing::debug!(
                        ?error,
                        "realm unavailable during Detached lifecycle dispatch (routine when a \
                         window close already delivered it)"
                    );
                }
            }));
        });

        // Window close -> close THIS window's own presentation before the
        // platform decides whether to exit. Load-bearing ordering, not just
        // bookkeeping hygiene: the winit backend's `CloseRequested` handling
        // calls a window's `on_close` (this callback) BEFORE it consults the
        // exit-policy hook `install_exit_policy_hook` installs above
        // (`AppRuntime::should_exit`, which decides purely from
        // `AppRuntime`'s own realm registry, never this backend's native
        // window count). Without this call, closing this app's only window
        // would leave the realm registry non-empty forever — nothing else
        // ever removes it — and the exit-policy hook would report "don't
        // exit" on every subsequent window close, including the very last
        // one, silently hanging the app open with no window left at all.
        // `close_this_window` (not `uninstall_platform_realm` directly): the
        // primary window closing while a `WindowPolicy::SharedRealm` sibling
        // survives must remove only THIS presentation, never the whole
        // realm out from under that sibling; `close_this_window` reduces to
        // the same full-realm-uninstall effect exactly when this is the
        // realm's sole presentation (today's single-window desktop shape).
        // The tail `teardown_platform_realm()` call still runs once
        // `Platform::run` actually returns, for the clipboard/redraw-window
        // cleanup no per-window close performs.
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
            close_this_window(realm_dispatch);
        }));

        // Window should-close -> allow by default
        window.on_should_close(Box::new(|| {
            tracing::debug!("Window close requested, allowing");
            true
        }));

        // Window focus/visibility -> the `(visible, focused)`
        // `AppLifecycleState` derivation. `on_visibility_status_change`
        // rides winit's `Occluded` event, which winit 0.30 only emits on
        // X11/macOS/iOS/Web (see that callback's own doc, verified against
        // winit's source) — winit has NO Wayland emitter for this event at
        // all, so on this workspace's own Wayland desktop reference the
        // window is always treated as visible (the same as before this
        // callback existed).
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

        // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed or requesting the initial redraw.
        // Both of those can synchronously run the first frame through
        // `dispatch_platform_realm`; if the slot were still empty at that
        // point, anything resolving it during that frame would silently
        // no-op instead of waking the loop.
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        // Mark lifecycle as started (Resumed). Routed through the same
        // dispatch every other lifecycle signal uses -- one fact, one place
        // (`emit_lifecycle_transition` reads the realm's own scheduler) --
        // rather than reaching for a process-global one that no longer
        // exists. A fresh realm's scheduler already starts at `Resumed`
        // (`BindingState::lifecycle_state`'s default), so this ladder is
        // empty and the call is a documented no-op, matching prior behavior.
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "desktop bootstrap must run on the realm's owner thread"
        );
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        );

        // 10. Request initial redraw, now that the window is stored.
        // `wake` (not a direct `request_redraw()` on the window): it clones
        // the window out from under the redraw-poke slot's lock before
        // calling through, so a backend whose `request_redraw` re-enters
        // this runtime synchronously (headless, in this crate's own tests)
        // cannot deadlock on that same lock — the same clone-then-call
        // discipline used by direct platform capabilities.
        wake();

        tracing::info!("Desktop platform initialized with callbacks");
        Ok(())
    }

    // Window creation, GPU/renderer setup, and callback wiring all run
    // inside `on_ready` rather than before `run()`. The winit backend can
    // only create a window from inside a running event loop (`ActiveEventLoop`
    // is unreachable beforehand); opening it earlier would deadlock forever
    // waiting for a pump that never started. `on_ready` runs exactly once,
    // synchronously, on this same thread — see `Platform::run`'s doc.
    //
    // The owner-host clear guard is armed HERE, before `run(...)`, not
    // inside `on_ready` — so a panic anywhere inside `on_ready` (or later,
    // on backends where `run` keeps running after it) unwinds through the
    // guard and cannot leak the host onto this thread past this call
    // (ADR-0039 §6).
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner);
        bootstrap_desktop(root, config, worker_reload, rebuild_registration_slot)
    }));

    // Event loop exited: drop the runtime now (releases the at-most-one
    // claim; outstanding senders turn `OwnerGone`) instead of at thread
    // death.
    drop(rebuild_registration.borrow_mut().take());
    teardown_platform_realm();

    // Surface a fatal bootstrap failure (GPU init, `UiRealm` construction,
    // root widget attach, or window creation) now that the event loop has
    // exited — `on_ready`'s `Err` propagates straight out of `Platform::run`;
    // no side-channel cell is needed to thread it out anymore.
    if let Err(err) = result {
        panic!("desktop bootstrap failed: {err:?}");
    }
}

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
// (see this module's own `use super::runtime::WindowPolicy` above). A
// narrower cfg here (e.g. `not(target_os = "ios")` alone) leaves this type
// annotation referencing an import that does not exist on android/wasm32,
// a hard compile error there, not merely dead code.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
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
    static PENDING_SECONDARY_WINDOW_COMPLETIONS: std::cell::RefCell<
        Vec<(WindowPolicy, Arc<dyn flui_platform::traits::PlatformWindow>)>,
    > = const { std::cell::RefCell::new(Vec::new()) };
}

/// Applies every `open_secondary_window` `Pending`-arm completion queued by
/// [`spawn_pending_secondary_window_completion`]'s own future, in request
/// order. Call only from a point where this thread's dispatch/hot-restart-
/// visit checkout state is already clear (`dispatched_realm_id` and
/// `iterating_all_realms` both settled back to their idle values) — the same
/// discipline [`super::runtime::AppRuntime::drain_pending_realm_mutations`]
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
fn drain_pending_secondary_window_completions() {
    let pending =
        PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    for (policy, window) in pending {
        if let Err(error) = finish_open_secondary_window(policy, window) {
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
fn open_secondary_window_impl(
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
        WindowOpen::Ready(window) => finish_open_secondary_window(policy, window).map(Some),
        WindowOpen::Pending(pending) => {
            spawn_pending_secondary_window_completion(policy, pending)?;
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
                    queue.borrow_mut().push((policy, window));
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
            let ui_realm = super::ui_realm::UiRealm::new(
                Arc::clone(&wake),
                Arc::clone(&window),
                scale_factor,
                runtime_needs_redraw_handle(),
            )
            .map_err(|error| {
                anyhow::anyhow!(error).context("secondary UiRealm construction failed")
            })?;
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
    window.on_should_close(Box::new(|| {
        tracing::debug!("Secondary window close requested, allowing");
        true
    }));
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

// ============================================================================
// Android Implementation
// ============================================================================

/// Run a FLUI application on Android with default configuration.
///
/// This is the primary entry point for Android apps. Call this from your
/// `android_main()` function:
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     flui_app::run_app_android(app, MyRootView);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android<V>(app: android_activity::AndroidApp, root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_android_with_config(app, root, AppConfig::default());
}

/// Run a FLUI application on Android with custom configuration.
///
/// Like [`run_app_android`] but allows specifying app configuration.
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     let config = AppConfig::new()
///         .with_title("My App")
///         .with_size(800, 600);
///     flui_app::run_app_android_with_config(app, MyRootView, config);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android_with_config<V>(app: android_activity::AndroidApp, root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    let _installation = super::logging::init_managed_logging(&config);

    tracing::info!(
        title = %config.title,
        "Starting FLUI application on Android"
    );

    run_android(root, config, app);
}

#[cfg(target_os = "android")]
fn run_android<V>(root: V, config: AppConfig, app: android_activity::AndroidApp)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{path::PathBuf, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_platform::{
        AndroidPlatform, Platform, WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use super::hot_reload::ScenePlugin;

    tracing::info!("Starting Android platform via flui-platform");

    // Hot-reload: build plugin path from app's internal data directory
    let plugin_path: PathBuf = app
        .internal_data_path()
        .map(|p| p.join("libflui_scene.so"))
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/libflui_scene.so"));

    // Inert unless this build carries the `hot-reload` feature.
    let hot_reload = ScenePlugin::new(&plugin_path);

    let platform: Box<dyn Platform> = Box::new(AndroidPlatform::new(app));

    /// The actual Android bootstrap: window, GPU, realm, and callback
    /// wiring. Runs once, synchronously, inside `on_ready` — which this
    /// backend delivers at the first `Resume` (module doc,
    /// `platforms/android/mod.rs:13`: "Resumed -> on_ready() -> create
    /// surface"). Migrated here from before `run()` (ADR-0039 slice 2):
    /// `on_ready` is `FnOnce` and fires exactly once, matching today's
    /// once-only pre-run bootstrap semantics exactly — no behavior change
    /// is intended or made on the subsequent-Resume/surface-recreation
    /// path, which flows through the backend's existing window/surface
    /// code untouched by this migration. **Unvalidated on-device**: no
    /// device and no CI compile target for `target_os = "android"` verify
    /// this; stated here and in the registry rather than assumed.
    ///
    /// Returns `Err` on bootstrap failure — `on_ready` itself is fallible
    /// now, so the Android backend's `run` loop stops (and propagates the
    /// error out) instead of continuing to pump input/frame
    /// dispatch for an app that never finished bootstrapping.
    fn bootstrap_android<V>(
        root: V,
        config: AppConfig,
        hot_reload: ScenePlugin,
    ) -> anyhow::Result<()>
    where
        V: View + StatelessView + Clone + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_android runs only after install_owner_platform")
        }

        // 0. Wire the platform clipboard (ADR-0034).
        let clipboard = owner_platform_installed(|owner| owner.shared().clipboard());
        APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));

        // 1. Open window (wraps the existing ANativeWindow). `Ready` is
        // guaranteed inside `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let window = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Failed to create Android window");
                return Err(anyhow::Error::from(error).context("Failed to create Android window"));
            }
        };

        // 2. Create GPU renderer (Vulkan backend on Android)
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(Renderer::new(window.as_ref()));
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return Err(anyhow::anyhow!(e).context("GPU init failed"));
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget (used when no plugin is active) at the
        // LOGICAL size; the paint root's DPR transform maps to physical.
        // `UiRealm::new` applies the DPR to the freshly built pipeline
        // before returning.
        let scale_factor = window.scale_factor() as f32;
        let wake = runtime_wake_callback();
        let ui_realm = match super::ui_realm::UiRealm::new(
            Arc::clone(&wake),
            Arc::clone(&window),
            scale_factor,
            runtime_needs_redraw_handle(),
        ) {
            Ok(realm) => realm,
            Err(error) => {
                tracing::error!(%error, "UiRealm construction failed");
                return Err(anyhow::anyhow!(error).context("UiRealm construction failed"));
            }
        };

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_realm.set_performance_overlay(config.show_performance_overlay);

        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            realm.attach_root_widget_with_size(
                &root,
                logical.width.0 as f32,
                logical.height.0 as f32,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
        }
        let realm_dispatch = install_platform_realm(ui_realm, &window);

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // Install the registration-lifetime surface applier alongside the
        // realm (cleared together at teardown) — see the desktop bootstrap's
        // matching comment for the take/call/restore protocol this feeds.
        {
            let renderer_resize = Arc::clone(&renderer);
            install_surface_applier(
                realm_dispatch.address.realm_id,
                move |size, scale_factor| {
                    let w = (size.width.0 * scale_factor) as u32;
                    let h = (size.height.0 * scale_factor) as u32;
                    renderer_resize.lock().resize(w, h);
                },
            );
        }

        // 5. Register input callback -> entered realm input dispatch
        window.on_input(Box::new(move |input: PlatformInput| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Input(input)),
            );
            DispatchEventResult::resolved(false, true)
        }));

        // 6. Register frame callback -- with hot-reload plugin override
        let renderer_frame = Arc::clone(&renderer);
        let hot_reload_frame = hot_reload.clone();
        window.on_request_frame(Box::new(move || {
            let renderer_frame = Arc::clone(&renderer_frame);
            let hot_reload_frame = hot_reload_frame.clone();
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Frame(Box::new(move |realm| {
                    // Owner-inbox drain: commands and worker results commit HERE,
                    // at the frame boundary while the scheduler phase is Idle —
                    // never inside the frame transaction below. Runs before
                    // everything else in this callback, including the hot-reload
                    // plugin scene fast path below, so a command-driven redraw
                    // request is observed by the very frame its wake produced
                    // regardless of which rendering path this frame takes.
                    let inbox_redraw = drain_owner_inbox(realm);

                    let mut r = renderer_frame.lock();
                    let (w, h) = r.size();

                    // If a scene plugin is live it owns this presentation frame,
                    // but the callback still executes inside the realm entry
                    // scope. Always `false` in a build without the `hot-reload`
                    // feature.
                    if hot_reload_frame.try_render_frame(&mut *r, w as f32, h as f32) {
                        return;
                    }
                    drop(r);

                    let has_pending = realm.has_pending_work();
                    let dirty = inbox_redraw || realm.needs_redraw() || has_pending;
                    let scheduler = realm.scheduler();
                    match wake_action(scheduler.frames_enabled(), dirty, scheduler.is_frame_scheduled())
                    {
                        WakeAction::Skip => return,
                        WakeAction::PumpAsync => {
                            // Frames disabled: pump only the async driver — no
                            // begin/draw frame, no tickers, no pipeline, no
                            // present. See `wake_action`'s doc for why this is
                            // the only thing keeping a spawned future
                            // progressing while backgrounded.
                            //
                            // `finish_async_pump` MUST run first, not after —
                            // see `UpdateScheduler::finish_async_pump`'s doc for the
                            // starvation hazard this ordering avoids.
                            scheduler.finish_async_pump();
                            scheduler.drive_async_tasks();
                            // Unconditional throttle: a self-re-arming task has
                            // no vsync/present call to bound it here either, and
                            // this arm has no gate-open signal to make the pace
                            // conditional the way desktop's does — see
                            // `NO_PRESENT_FALLBACK_PACE`'s doc.
                            std::thread::sleep(NO_PRESENT_FALLBACK_PACE);
                            return;
                        }
                        WakeAction::Render => {}
                    }

                    let now = web_time::Instant::now();
                    // UpdateScheduler callbacks and rendering share ONE `UiRealm::enter`
                    // dynamic extent; callbacks may legally resolve realm-local
                    // capabilities throughout the complete frame transaction.
                    scheduler.drive_frame_with_lane(now, flui_scheduler::IdleDeadline::far_future(now), || {
                        let mut r = renderer_frame.lock();
                        realm.render_frame_entered(&mut *r);

                        if r.is_device_lost() {
                            match pollster::block_on(r.recover()) {
                                Ok(()) => {
                                    tracing::warn!("GPU device lost — recovered successfully");
                                    realm.wake_frame();
                                }
                                Err(e) => {
                                    tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                                }
                            }
                        }
                    }, realm.local_post_frame_lane());
                })),
            );
        }));

        // 7. Register resize callback -> typed Resized event; the applier
        // installed above (not this closure) actually touches the renderer.
        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
            );
        }));

        // 8. Lifecycle callbacks
        //
        // Detached is realm-dispatched so interrupted gesture state is drained
        // before lifecycle observers run.

        // Platform quit -> Detached (frames disabled, listeners notified).
        owner_platform_installed(|owner| {
            owner.shared().on_quit(Box::new(move || {
                tracing::info!("Platform quit");
                debug_assert_eq!(
                    std::thread::current().id(),
                    realm_dispatch.owner_thread,
                    "platform on_quit must fire on the realm's owner thread"
                );
                if let Err(error) = dispatch_platform_realm(
                    realm_dispatch,
                    RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Detached)),
                ) {
                    // Trace-only: the scheduler died WITH the realm now (each
                    // realm owns its own), so there is no process-global
                    // scheduler left to notify as a fallback.
                    tracing::warn!(
                        ?error,
                        "realm unavailable during Detached lifecycle dispatch"
                    );
                }
            }));
        });

        // Window close (fired by Android Destroy event)
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
        }));

        // Window active status. On Android this one callback conflates real
        // window focus (`MainEvent::GainedFocus`/`LostFocus`) with the app's
        // actual pause/resume signal (`MainEvent::Resume`/`Pause` currently fire
        // the identical `dispatch_active_status_change` — see
        // `flui-platform`'s `platforms/android/mod.rs`); a dedicated
        // `MainEvent` -> lifecycle callback that tells them apart is a named
        // follow-up (ADR-0035), not this PR. Until that split lands, this keeps
        // the existing transport but fixes the mapping: `false` ladders all the
        // way to `Paused` and `true` back to `Resumed` — Android's
        // backgrounding signal needs the deeper ladder the desktop/web
        // `(visible, focused)` derivation (which only ever reaches
        // `Inactive`/`Hidden`) does not produce.
        window.on_active_status_change(Box::new(move |resumed| {
            let target = if resumed {
                AppLifecycleState::Resumed
            } else {
                AppLifecycleState::Paused
            };
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Frame(Box::new(move |realm| {
                    let old = realm.scheduler().lifecycle_state();
                    emit_lifecycle_transition(realm, old, target);
                })),
            );
        }));

        // 9. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed or requesting the initial redraw.
        // Both of those can synchronously run the first frame through
        // `dispatch_platform_realm`; if the slot were still empty at that
        // point, anything resolving it during that frame would silently
        // no-op instead of waking the loop.
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        // Mark lifecycle as started (Resumed). Routed through dispatch --
        // see `run_desktop`'s matching comment for why.
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "android bootstrap must run on the realm's owner thread"
        );
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        );

        // 10. Request initial redraw, now that the window is stored.
        wake();

        tracing::info!("Android platform initialized with callbacks (hot-reload enabled)");
        Ok(())
    }

    // Owner-host clear guard armed BEFORE `run(...)`, not inside `on_ready`
    // (ADR-0039 §6) — see `run_desktop`'s matching comment.
    let _owner_host_clear_guard = OwnerHostClearGuard::arm();
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner);
        bootstrap_android(root, config, hot_reload)
    }));
    teardown_platform_realm();

    // `on_ready`'s `Err` propagates straight out of `Platform::run`; surface
    // it the same way `run_desktop` does now that the event loop has
    // exited.
    if let Err(err) = result {
        panic!("android bootstrap failed: {err:?}");
    }
}

// ============================================================================
// iOS Implementation
// ============================================================================

#[cfg(target_os = "ios")]
fn run_ios(_config: AppConfig) {
    // Native iOS (UIKit windowing + surface) is a Cross.P (Platform breadth)
    // deliverable — see docs/ROADMAP.md's Cross.P section. This stub exists
    // only so `#[cfg(target_os = "ios")]` builds compile; there is no
    // UIKit-backed `flui-platform` implementation to call into yet.
    tracing::info!("iOS platform - not yet implemented");
}

// ============================================================================
// Web Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
fn run_web<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::wgpu::Renderer;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    tracing::info!("Starting web platform via flui-platform");

    // Platform init is an environment failure (unsupported browser, missing
    // wasm feature, driver problem), not a `BUG:` invariant — see the
    // matching comment in `run_desktop` above for why this is a `match` +
    // `panic!` with a full error log instead of a bare `.expect()`.
    let platform = match flui_platform::current_platform() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "Failed to initialize platform");
            panic!("web bootstrap failed: platform initialization error: {error:?}");
        }
    };

    /// The actual web bootstrap: canvas window, renderer, realm, and
    /// callback wiring. Runs once, synchronously, inside `on_ready` —
    /// `WebPlatform::run` invokes it before starting the RAF loop
    /// (ADR-0039 slice 2 migration; behavior-preserving, since `on_ready`
    /// already runs synchronously on this thread before `run` returns).
    ///
    /// Returns `Err` on bootstrap failure — `on_ready` itself is fallible
    /// now, so `WebPlatform::run` does not install the RAF loop over a
    /// half-built page.
    fn bootstrap_web<V>(root: V, config: AppConfig) -> anyhow::Result<()>
    where
        V: View + StatelessView + Clone + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            with_owner_platform(f)
                .expect("BUG: bootstrap_web runs only after install_owner_platform")
        }

        // 0. Wire the platform clipboard (ADR-0034).
        let clipboard = owner_platform_installed(|owner| owner.shared().clipboard());
        APP_RUNTIME.with(|slot| slot.borrow().set_platform_clipboard(clipboard));

        // 1. Open window (creates canvas). `Ready` is guaranteed inside
        // `on_ready` (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let window = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Failed to create canvas window");
                return Err(anyhow::Error::from(error).context("Failed to create canvas window"));
            }
        };

        // 2. Shared renderer slot — starts as None, filled async once the WebGPU
        //    adapter is available. `Option` lets the frame callback skip frames that
        //    arrive before the renderer is ready.
        let renderer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));

        let phys_size = window.physical_size();
        let renderer_init = Arc::clone(&renderer);
        let renderer_window = Arc::clone(&window);

        // The future owns a strong window reference. This is required because the
        // browser platform installs RAF and returns immediately, and startup can
        // also return early before the window reaches AppRuntime's redraw-poke slot.
        wasm_bindgen_futures::spawn_local(async move {
            let mut r = match Renderer::new(renderer_window.as_ref()).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("GPU init failed: {:?}", e);
                    return;
                }
            };
            r.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);
            tracing::info!("WebGPU renderer initialized");
            *renderer_init.lock() = Some(r);
        });

        // 3. Mount root widget at the LOGICAL size; the paint root's DPR
        // transform maps to the physical canvas. `UiRealm::new` applies the
        // DPR to the freshly built pipeline before returning.
        let scale_factor = window.scale_factor() as f32;
        let wake = runtime_wake_callback();
        let ui_realm = match super::ui_realm::UiRealm::new(
            Arc::clone(&wake),
            Arc::clone(&window),
            scale_factor,
            runtime_needs_redraw_handle(),
        ) {
            Ok(realm) => realm,
            Err(error) => {
                tracing::error!(%error, "UiRealm construction failed");
                return Err(anyhow::anyhow!(error).context("UiRealm construction failed"));
            }
        };

        // Debug overlay: `Some` stats IS the enable flag, so this is the
        // single point that turns the frame path's overlay work on.
        ui_realm.set_performance_overlay(config.show_performance_overlay);

        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            realm.attach_root_widget_with_size(
                &root,
                logical.width.0 as f32,
                logical.height.0 as f32,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            return Err(anyhow::anyhow!(e).context("Root widget attach failed"));
        }
        let realm_dispatch = install_platform_realm(ui_realm, &window);

        // Install the registration-lifetime surface applier alongside the
        // realm (cleared together at teardown) — see the desktop bootstrap's
        // matching comment for the take/call/restore protocol this feeds.
        {
            let renderer_resize = Arc::clone(&renderer);
            install_surface_applier(
                realm_dispatch.address.realm_id,
                move |size, scale_factor| {
                    if let Some(renderer) = renderer_resize.lock().as_mut() {
                        let width = (size.width.0 * scale_factor) as u32;
                        let height = (size.height.0 * scale_factor) as u32;
                        renderer.resize(width, height);
                    }
                },
            );
        }

        // 4. Register input callback
        window.on_input(Box::new(move |input: PlatformInput| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Input(input)),
            );
            DispatchEventResult::resolved(false, true)
        }));

        // 5. Register frame callback
        let renderer_frame = Arc::clone(&renderer);
        window.on_request_frame(Box::new(move || {
            let renderer_frame = Arc::clone(&renderer_frame);
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Frame(Box::new(move |realm| {
                    // Owner-inbox drain: commands and worker results commit HERE,
                    // at the frame boundary while the scheduler phase is Idle —
                    // never inside the frame transaction below. Runs before the
                    // dirty gate so a command-driven redraw request is observed
                    // by the very frame its wake produced.
                    let inbox_redraw = drain_owner_inbox(realm);

                    let has_pending = realm.has_pending_work();
                    let dirty = inbox_redraw || realm.needs_redraw() || has_pending;
                    let scheduler = realm.scheduler();
                    match wake_action(scheduler.frames_enabled(), dirty, scheduler.is_frame_scheduled())
                    {
                        WakeAction::Skip => return,
                        WakeAction::PumpAsync => {
                            // Frames disabled: pump only the async driver — see
                            // `wake_action`'s doc for why this is the only thing
                            // keeping a spawned future progressing while
                            // backgrounded.
                            //
                            // `finish_async_pump` MUST run first, not after —
                            // see `UpdateScheduler::finish_async_pump`'s doc for the
                            // starvation hazard this ordering avoids.
                            //
                            // No `NO_PRESENT_FALLBACK_PACE` sleep here, unlike
                            // desktop/Android: this callback is driven by the
                            // browser's `requestAnimationFrame` loop
                            // (`start_raf_loop`, `flui-platform`'s web backend),
                            // which fires unconditionally once per animation
                            // frame regardless of whether a redraw was
                            // requested — the browser's own vsync-paced RAF
                            // cadence already bounds this arm's re-wake rate, so
                            // an additional sleep would be redundant. It would
                            // also be unsound here: `wasm32-unknown-unknown` has
                            // no real OS threads, and blocking the single JS
                            // thread with `std::thread::sleep` would hang the
                            // page rather than pace it.
                            scheduler.finish_async_pump();
                            scheduler.drive_async_tasks();
                            return;
                        }
                        WakeAction::Render => {}
                    }

                    let now = web_time::Instant::now();
                    // UpdateScheduler callbacks and rendering share one realm entry.
                    scheduler.drive_frame_with_lane(now, flui_scheduler::IdleDeadline::far_future(now), || {
                        let mut slot = renderer_frame.lock();
                        let Some(r) = slot.as_mut() else {
                            return;
                        };

                        realm.render_frame_entered(r);

                        if r.is_device_lost() {
                            drop(slot);
                            let renderer_recover = Arc::clone(&renderer_frame);
                            // A cloned, `'static` wake handle: the spawned
                            // future outlives this callback's `&UiRealm`
                            // borrow, so it cannot capture `realm` itself.
                            let wake = realm.wake_handle();
                            wasm_bindgen_futures::spawn_local(async move {
                                // Never hold the renderer mutex across `.await`.
                                let Some(mut renderer) = renderer_recover.lock().take() else {
                                    return;
                                };
                                let result = renderer.recover().await;
                                *renderer_recover.lock() = Some(renderer);
                                match result {
                                    Ok(()) => {
                                        tracing::warn!("GPU device lost — recovered successfully");
                                        wake();
                                    }
                                    Err(e) => {
                                        tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                                    }
                                }
                            });
                        }
                    }, realm.local_post_frame_lane());
                })),
            );
        }));

        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
            );
        }));

        // 6. Lifecycle callbacks
        //
        // Detached is realm-dispatched so interrupted gesture state is drained
        // before lifecycle observers run.
        owner_platform_installed(|owner| {
            owner.shared().on_quit(Box::new(move || {
                tracing::info!("Web platform quit");
                debug_assert_eq!(
                    std::thread::current().id(),
                    realm_dispatch.owner_thread,
                    "platform on_quit must fire on the realm's owner thread"
                );
                if let Err(error) = dispatch_platform_realm(
                    realm_dispatch,
                    RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Detached)),
                ) {
                    // Trace-only: the scheduler died WITH the realm now (each
                    // realm owns its own), so there is no process-global
                    // scheduler left to notify as a fallback.
                    tracing::warn!(
                        ?error,
                        "realm unavailable during Detached lifecycle dispatch"
                    );
                }
            }));
        });

        window.on_close(Box::new(move || {
            tracing::info!("Canvas window closed");
            // On web, no explicit quit mechanism needed
        }));

        // No `on_visibility_status_change` registration on web (yet): there is
        // no occlusion signal wired for this backend in this PR (winit's
        // `Occluded` is desktop-only) — a DOM `visibilitychange` listener is a
        // future follow-up, not this PR's scope.
        window.on_active_status_change(Box::new(move |focused| {
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmTask::Event(PlatformToUi::WindowFocus(focused)),
            );
        }));

        // 7. Store the window in AppRuntime's redraw-poke slot — BEFORE
        // marking the lifecycle Resumed, which can synchronously run the
        // first frame through `dispatch_platform_realm`; anything resolving
        // the slot during that frame must not see it empty.
        APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));

        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "web bootstrap must run on the realm's owner thread"
        );
        // Routed through dispatch -- see `run_desktop`'s matching comment.
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Lifecycle(AppLifecycleState::Resumed)),
        );

        tracing::info!("Web platform initialized with callbacks");
        Ok(())
    }

    // Run the event loop (takes ownership of the platform). No
    // `OwnerHostClearGuard` here — deliberately: `WebPlatform::run` installs
    // the RAF callback and returns immediately, and tearing down the realm
    // (or the owner host) at that point would destroy it before the first
    // frame. The host stays owner-TLS resident for the page's lifetime
    // (ADR-0039 §6/§7 "wasm posture"). An explicit web detach/quit
    // ownership hook is deferred until the platform exposes a callback
    // whose lifetime encloses the RAF registration.
    let result = platform.run(Box::new(move |owner| {
        install_owner_platform(owner);
        bootstrap_web(root, config)?;
        tracing::info!("Web platform ready");
        Ok(())
    }));

    // `on_ready`'s `Err` propagates straight out of `Platform::run`:
    // `WebPlatform::run` does not install the RAF loop over a half-built
    // page in that case.
    if let Err(err) = result {
        panic!("web bootstrap failed: {err:?}");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_types::geometry::px;
    use flui_view::{BuildContext, IntoView, View, ViewExt};

    use super::*;

    /// Trivial leaf fixture: an empty view used as the terminal node under
    /// `OwnerLocalRoot` below, and constructible on its own wherever a test
    /// needs a minimal `View + StatelessView` root.
    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            TestView.boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[derive(Clone)]
    struct OwnerLocalRoot {
        value: Rc<Cell<usize>>,
    }

    impl StatelessView for OwnerLocalRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.value.set(self.value.get() + 1);
            TestView.boxed()
        }
    }

    impl View for OwnerLocalRoot {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn runner_entrypoints_accept_owner_local_root_state() {
        static_assertions::assert_not_impl_any!(OwnerLocalRoot: Send, Sync);

        std::hint::black_box(run_app_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot));
        std::hint::black_box(
            run_app_with_config_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot, AppConfig),
        );
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, px(800.0));
    }

    /// Bootstrap ordering invariant shared by `bootstrap_desktop`, `run_android`,
    /// and `run_web`: the window must be stored in `AppRuntime`'s redraw-poke
    /// slot before anything that could synchronously observe it (the initial
    /// redraw request, `Lifecycle::Started`) runs — otherwise the first such
    /// observer would silently see nothing installed.
    ///
    /// `bootstrap_desktop`/`run_android`/`run_web` themselves cannot run in a
    /// unit test: each opens its window from inside a live platform event loop
    /// (`ActiveEventLoop` is unreachable outside `Platform::run`) and creates a
    /// real GPU `Renderer`, gated behind the separate `enable-wgpu-tests` CI job
    /// (WARP), not this one. This instead drives the exact ordering invariant
    /// headlessly: `HeadlessWindow::request_redraw` (flui-platform's headless
    /// backend, used elsewhere in this crate's tests) dispatches its
    /// `on_request_frame` callback SYNCHRONOUSLY — unlike a real winit window,
    /// where a queued `RedrawRequested` would not fire until `on_ready` (and
    /// this reordering) has already returned. That is exactly why the ordering
    /// bug was invisible in a real window's actual first frame but is directly
    /// observable here.
    ///
    /// Checks a unique window *size* rather than mere `is_some()`, so this
    /// cannot pass merely because an earlier test left SOME window installed
    /// — only THIS test's window, with THIS test's unmistakable marker size,
    /// proves `set_redraw_window` ran before the callback.
    ///
    /// If reverted: swap the order of the two calls below (request the
    /// redraw, then store the window — the pre-fix shape) and this fails:
    /// `wake_frame` finds no window yet, never calls `request_redraw` on it,
    /// and the callback never fires at all.
    ///
    /// No test lock: this touches `APP_RUNTIME`, a `thread_local!`, and the
    /// standard library test harness runs each `#[test]` on its own freshly
    /// spawned thread, so a fresh `AppRuntime` (no realm, no owner platform)
    /// is what this test's thread starts from regardless of what any other
    /// concurrently-running test does on ITS OWN thread — the same reasoning
    /// this file's other thread-local-only tests below rely on. The retired
    /// `AppBinding`-era version of this test carried a dedicated per-test
    /// window-identity lock, and later, briefly, `UpdateScheduler` carried a
    /// sibling per-test scheduler-phase lock; both are deleted now, not
    /// ported forward, because the state each one guarded
    /// (`AppBinding::instance()`'s active window, and the process-global
    /// half of the `UpdateScheduler` singleton respectively) no longer exists —
    /// `AppBinding` is gone entirely and every `UiRealm` owns its own fresh
    /// `UpdateScheduler` value — and because a per-test-thread thread-local needs
    /// no cross-test lock in the first place.
    #[test]
    fn desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let marker_size = flui_types::Size::new(px(4001.0), px(4002.0));

        let platform = flui_platform::headless_platform();
        let window = platform
            .open_window(flui_platform::traits::WindowOptions {
                size: marker_size,
                ..Default::default()
            })
            .expect("headless platform always opens a window");

        // `on_request_frame` requires `Send` on the callback; `AppRuntime` is
        // not `Send` (it holds owner-thread-affine realm state), so the
        // closure below cannot capture a specific `&AppRuntime`. Resolving
        // `APP_RUNTIME` fresh inside the closure (zero captures for the
        // runtime itself) sidesteps that entirely.
        //
        // Reads through `with_redraw_window`, NOT `wake_frame`/`request_redraw`:
        // a headless window's `request_redraw` dispatches this very callback
        // synchronously, so calling anything that re-locks the redraw-poke
        // slot from in here (the two are on the same thread, same call
        // stack) would deadlock on the slot's own non-reentrant lock.
        let saw_marker_window = Arc::new(AtomicBool::new(false));
        let saw_marker_window_cb = Arc::clone(&saw_marker_window);
        window.on_request_frame(Box::new(move || {
            let matches_marker = APP_RUNTIME
                .with(|slot| {
                    slot.borrow()
                        .with_redraw_window(|w| w.bounds().size == marker_size)
                })
                .unwrap_or(false);
            saw_marker_window_cb.store(matches_marker, Ordering::SeqCst);
        }));

        // Mirrors the FIXED order in `bootstrap_desktop`/`run_android`:
        // store the window BEFORE requesting the initial redraw. `wake_frame`
        // (not a direct `request_redraw()` on the window) clones the window
        // out from under the lock before calling through, so this call
        // cannot deadlock against the callback's own `with_redraw_window`
        // re-entry above.
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            state.set_redraw_window(window);
            state.wake_frame();
        });

        assert!(
            saw_marker_window.load(Ordering::SeqCst),
            "set_redraw_window must have taken effect before the initial redraw \
             fires the frame callback that could read the redraw-poke slot",
        );
        // Clean up so this test's window does not linger for whatever test
        // runs next on this pool thread.
        APP_RUNTIME.with(|slot| slot.borrow().clear_redraw_window());
    }

    // ========================================================================
    // Owner-platform host tests (ADR-0039 §6)
    // ========================================================================

    #[test]
    fn owner_platform_host_installs_and_clears_around_run() {
        use flui_platform::headless_platform;

        assert!(
            with_owner_platform(|_| ()).is_none(),
            "no host installed before any on_ready has run on this thread"
        );

        // `PlatformReadyCallback` is `Box<dyn FnOnce(OwnerPlatform) + 'static>`,
        // so the closure below cannot borrow a stack-local `Cell` — `Rc`
        // gives it an owned handle instead (single-threaded: headless `run`
        // invokes `on_ready` synchronously, on this same thread).
        let seen_while_installed = Rc::new(Cell::new(false));
        let seen_while_installed_for_closure = Rc::clone(&seen_while_installed);
        {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            let result = platform.run(Box::new(move |owner| {
                install_owner_platform(owner);
                let observed = with_owner_platform(|_owner| true);
                seen_while_installed_for_closure.set(observed == Some(true));
                Ok(())
            }));
            assert!(result.is_ok(), "on_ready returns Ok here");
        } // `_clear_guard` drops here.

        assert!(
            seen_while_installed.get(),
            "the accessor must yield Some(_) while a host is installed"
        );
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "the clear guard must remove the host once its scope ends"
        );
    }

    /// `install_owner_platform` alone -- the exact path `run_direct` takes,
    /// which opens a window but never installs a `UiRealm` -- must NOT
    /// resolve `SharedEngineServices`. Only `install_platform_realm`
    /// (exercised by the realm-install tests elsewhere in this file) does
    /// that, so a backend that never hosts a realm never pays for
    /// painting/semantics/scheduler singleton construction or full
    /// system-font enumeration it cannot use.
    #[test]
    fn install_owner_platform_alone_does_not_resolve_services() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let result = platform.run(Box::new(|owner| {
            install_owner_platform(owner);
            assert!(
                !APP_RUNTIME.with(|slot| slot.borrow().services_resolved()),
                "install_owner_platform alone must not resolve SharedEngineServices"
            );
            Ok(())
        }));
        assert!(result.is_ok(), "on_ready returns Ok here");
    }

    #[test]
    fn owner_platform_host_panic_in_on_ready_still_clears() {
        use flui_platform::headless_platform;

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            let _ = platform.run(Box::new(|owner| {
                install_owner_platform(owner);
                panic!("exercise on_ready panic cleanup");
            }));
        }));

        assert!(unwind.is_err(), "on_ready's panic must propagate");
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "a panic inside on_ready must still unwind through the clear guard \
             (armed before Platform::run, not inside on_ready) rather than \
             leaking the host onto this thread"
        );
    }

    /// `on_ready` returning `Err` must propagate all the way out of
    /// `Platform::run` — not be swallowed into
    /// a bare log while the loop keeps running a half-built app — AND the
    /// `AppRuntime.owner_platform` TLS clear guard (armed before `run`, per
    /// the existing unwind-safety contract) must still fire on this ordinary
    /// `Err` return, exactly as it does on a panic.
    #[test]
    fn owner_platform_host_on_ready_error_propagates_and_still_clears() {
        use flui_platform::headless_platform;

        let result = {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            platform.run(Box::new(|owner| {
                install_owner_platform(owner);
                assert!(
                    with_owner_platform(|_| ()).is_some(),
                    "the host is installed while on_ready runs, even on the \
                     path that is about to fail"
                );
                Err(anyhow::anyhow!("simulated bootstrap failure"))
            }))
        }; // `_clear_guard` drops here -- before the assertions below.

        assert!(
            result.is_err(),
            "on_ready's Err must propagate out of Platform::run, not be \
             swallowed"
        );
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "the clear guard must still remove the host on the Err path, \
             the same as it does on a panic"
        );
    }

    #[test]
    #[should_panic(
        expected = "with_owner_platform called while the installed realm's scheduler is inside"
    )]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the fence is a debug_assert!; release builds don't panic"
    )]
    fn owner_platform_accessor_fences_the_installed_realms_frame_transaction() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let window = headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window");
        let dispatcher =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);

        // A clone of the installed realm's OWN scheduler -- same underlying
        // `SchedulerInner` fence (c) reads through `installed_realm_phase`.
        // Driven directly here, with the realm still resident in its slot
        // (not checked out via `dispatch_platform_realm`) -- the sibling
        // test right below this one, `..._through_dispatch`, pins the other
        // half: the realm checked OUT for a dispatched task, where
        // `installed_realm_phase` must fall back to `dispatched_scheduler`
        // instead of reading the resident slot directly.
        let (scheduler, local_post_frame) = APP_RUNTIME.with(|slot| {
            let borrowed = slot.borrow();
            let realm = borrowed
                .realms
                .get(&dispatcher.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("just installed above");
            (
                realm.scheduler().clone(),
                realm.local_post_frame_lane().clone(),
            )
        });

        // `drive_frame` leaves the scheduler in `PersistentCallbacks` for the
        // duration of its `pipeline` closure -- a forbidden phase per fence
        // (c). A panicking pipeline is caught internally and resolved back
        // to `Idle` via `abort_frame()` before the panic resumes, so this
        // test's own `#[should_panic]` unwind leaves the scheduler clean.
        let now = web_time::Instant::now();
        scheduler.drive_frame_with_lane(
            now,
            flui_scheduler::IdleDeadline::far_future(now),
            || {
                let _ = with_owner_platform(|_owner| ());
            },
            &local_post_frame,
        );
    }

    /// The through-dispatch half of the fence-(c) pin above: `with_owner_platform`
    /// called from INSIDE a `RealmTask::Frame` running through
    /// `dispatch_platform_realm` -- not driven directly against a resident
    /// realm -- must still trip the debug_assert while a frame phase is
    /// active.
    ///
    /// Red before the `dispatched_scheduler` fallback existed:
    /// `dispatch_platform_realm` checks the realm's `UiRealm` OUT of its slot
    /// for the entire extent of the dispatched task (see its own doc), so
    /// `installed_realm_phase` reading only resident slots would observe
    /// `None` for this call, not `PersistentCallbacks` -- vacuously "not
    /// mid-frame",
    /// the debug_assert would pass, and this test would fail its
    /// `#[should_panic]` expectation. Green now because
    /// `dispatch_platform_realm` stashes a clone of the checked-out realm's
    /// scheduler into `dispatched_scheduler` before running the queued task,
    /// and `installed_realm_phase` falls back to it exactly when `realm`
    /// itself is empty.
    #[test]
    #[should_panic(
        expected = "with_owner_platform called while the installed realm's scheduler is inside"
    )]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the fence is a debug_assert!; release builds don't panic"
    )]
    fn owner_platform_accessor_fences_the_installed_realms_frame_transaction_through_dispatch() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let window = headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window");
        let dispatcher =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);

        // Unlike the sibling test above, this drives the frame from INSIDE a
        // `RealmTask::Frame` dispatched through `dispatch_platform_realm` --
        // the realm is checked out of its registry slot for the whole
        // closure below, exactly the window `dispatched_scheduler` exists to
        // cover. `drive_frame`'s `PersistentCallbacks` phase is active while
        // `with_owner_platform` is called, so the fence must trip here
        // exactly as it does when driven directly.
        let dispatch_result = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                let now = web_time::Instant::now();
                realm.scheduler().drive_frame_with_lane(
                    now,
                    flui_scheduler::IdleDeadline::far_future(now),
                    || {
                        let _ = with_owner_platform(|_owner| ());
                    },
                    realm.local_post_frame_lane(),
                );
            })),
        );
        // Unreachable on the fence-tripping path (the debug_assert panics
        // first, unwinding out of `dispatch_platform_realm` before it can
        // return) -- kept only so a build without debug assertions (where
        // this test is `ignore`d) still type-checks the dispatch call.
        let _ = dispatch_result;
    }

    /// Hot-restart survival (ADR-0039 §6): `owner_platform`
    /// is a loop-scoped `AppRuntime` field, deliberately not cleared by
    /// `teardown_platform_realm` alongside the realm-facing fields it DOES
    /// clear (`realm`, `queue`, `owner_thread`, `address`,
    /// `surface_applier`) -- tearing down a realm on the owner thread must
    /// not strand the loop's capability, because the loop may host a fresh
    /// realm next without ever calling `Platform::run` again (hot-restart
    /// does exactly this today, `install_platform_realm`).
    #[test]
    fn owner_platform_survives_realm_teardown() {
        use flui_platform::headless_platform;

        // `Rc<Cell<_>>`, not a bare local: the `on_ready` closure below is
        // `Box<dyn FnOnce(OwnerPlatform) + 'static>`, so it cannot borrow a
        // stack local -- see the sibling install/clear test's identical
        // note. `(bool, bool)` is `Copy`, so `Cell` suffices.
        let observed = Rc::new(Cell::new((false, false)));
        let observed_for_closure = Rc::clone(&observed);

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let result = platform.run(Box::new(move |owner| {
            install_owner_platform(owner);
            let before_teardown = with_owner_platform(|_owner| true) == Some(true);

            // Simulate hot-restart: a realm's teardown runs on this owner
            // thread while the loop keeps running (headless `run` returns
            // immediately either way, but the TLS host's contract does not
            // depend on that -- it is exercised identically whether the
            // loop is about to return or about to host another realm).
            teardown_platform_realm();

            let after_teardown = with_owner_platform(|_owner| true) == Some(true);
            observed_for_closure.set((before_teardown, after_teardown));
            Ok(())
        }));
        assert!(result.is_ok(), "on_ready returns Ok here");

        let (before_teardown, after_teardown) = observed.get();
        assert!(
            before_teardown,
            "the host must be installed before teardown runs"
        );
        assert!(
            after_teardown,
            "teardown_platform_realm must not clear AppRuntime.owner_platform -- \
             the loop may host another realm before it exits (hot-restart)"
        );
    }

    /// Regression pin for the "No host re-entry" rule on `with_owner_platform`'s
    /// own rustdoc: since `AppRuntime` folded the realm-facing state and
    /// `owner_platform` into one `RefCell`, a closure that calls back into
    /// any function touching that same cell while `with_owner_platform`
    /// still holds its immutable borrow is a guaranteed `BorrowMutError`
    /// panic. `dispatch_platform_realm` is the stand-in host op here; the
    /// same panic would fire for `install_platform_realm`,
    /// `teardown_platform_realm`, or `install_surface_applier` instead, for
    /// the identical reason (all of them `borrow_mut()` the same cell).
    #[test]
    // Substring match, not the full message: `RefCell`'s panic wording
    // ("already borrowed: BorrowMutError" vs. "already mutably borrowed:
    // BorrowError" depending on which side re-enters) has varied across
    // Rust versions and could vary again; "borrow" is the one substring
    // present in every variant, so this still fails on an unrelated panic
    // while staying stable across toolchains.
    #[should_panic(expected = "borrow")]
    fn with_owner_platform_reentering_dispatch_panics() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let _ = platform.run(Box::new(|owner| {
            install_owner_platform(owner);
            with_owner_platform(|_owner| {
                // Any host op re-entering here panics: `with_owner_platform`
                // still holds `APP_RUNTIME.borrow()` for the duration of
                // this closure, and `dispatch_platform_realm` immediately
                // tries `slot.borrow_mut()` on the very first line of its
                // own TLS access.
                let dispatcher = RealmDispatcher {
                    owner_thread: std::thread::current().id(),
                    address: flui_foundation::PresentationAddress {
                        realm_id: flui_foundation::RealmId::new_gen(
                            0,
                            std::num::NonZeroU32::new(1).unwrap(),
                        ),
                        presentation_id: flui_foundation::PresentationId::new_gen(
                            0,
                            std::num::NonZeroU32::new(1).unwrap(),
                        ),
                    },
                };
                let _ = dispatch_platform_realm(dispatcher, RealmTask::Frame(Box::new(|_| {})));
            });
            Ok(())
        }));
    }
}
