//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{StatelessView, View};

use super::AppConfig;

#[cfg(not(target_os = "ios"))]
use std::sync::Arc;
#[cfg(not(target_os = "ios"))]
use std::sync::atomic::AtomicBool;

#[cfg(not(target_os = "ios"))]
use flui_scheduler::AppLifecycleState;

#[cfg(not(target_os = "ios"))]
use super::runtime::AppRuntime;

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
///     bindings drive their own binding-local `Scheduler`, never a realm
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
/// scale_factor)`. Named so [`AppRuntime`]'s `surface_applier` field
/// declaration reads plainly instead of spelling out the boxed closure type
/// inline. `pub(super)` (rather than private) so [`AppRuntime`]'s struct
/// definition in the sibling `runtime` module can name this type.
#[cfg(not(target_os = "ios"))]
pub(super) type SurfaceApplier =
    Box<dyn FnMut(flui_types::Size<flui_types::geometry::Pixels>, f32)>;

/// Restores a taken [`SurfaceApplier`] back into [`APP_RUNTIME`]'s slot when
/// dropped — including during an unwinding drop, so a panic inside the
/// applier's own call (caught by `dispatch_platform_realm`'s outer
/// `catch_unwind`) cannot permanently strand resizing. Without this, the
/// applier taken out before the call is simply never restored once the call
/// panics, and every later `Resized` event finds the slot empty forever,
/// silently coalescing at the `None` arm's trace instead of ever applying
/// again.
#[cfg(not(target_os = "ios"))]
#[must_use = "dropping this immediately restores the applier with no call in between"]
struct SurfaceApplierRestoreGuard(Option<SurfaceApplier>);

#[cfg(not(target_os = "ios"))]
impl SurfaceApplierRestoreGuard {
    fn call(&mut self, size: flui_types::Size<flui_types::geometry::Pixels>, scale_factor: f32) {
        if let Some(applier) = self.0.as_mut() {
            applier(size, scale_factor);
        }
    }
}

#[cfg(not(target_os = "ios"))]
impl Drop for SurfaceApplierRestoreGuard {
    fn drop(&mut self) {
        if let Some(applier) = self.0.take() {
            APP_RUNTIME.with(|slot| {
                slot.borrow_mut().surface_applier = Some(applier);
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

/// One queued unit of owner-thread work: either a typed cross-thread
/// [`PlatformToUi`] event, or the co-located frame pump. `Frame` is
/// deliberately NOT part of the cross-thread vocabulary above — it is
/// same-thread by construction (the `WrongThread` guard in
/// [`dispatch_platform_realm`] rejects it from anywhere else) and carries an
/// owner-local closure, which a cross-thread payload must never do
/// (ADR-0037 §3 forbids `Box<dyn FnOnce()>` on that boundary). The `Send`
/// assertion above applies to `PlatformToUi` only; splitting the two types
/// is what makes that assertion non-vacuous.
// `pub(super)` (rather than private) so `AppRuntime`'s `queue` field, defined
// in the sibling `runtime` module, can name this type.
#[cfg(not(target_os = "ios"))]
pub(super) enum RealmTask {
    Event(PlatformToUi),
    Frame(Box<dyn FnOnce(&super::ui_realm::UiRealm)>),
}

#[cfg(not(target_os = "ios"))]
impl RealmTask {
    fn run(self, realm: &super::ui_realm::UiRealm) {
        match self {
            Self::Event(event) => event.run(realm),
            Self::Frame(run) => run(realm),
        }
    }
}

#[cfg(not(target_os = "ios"))]
impl PlatformToUi {
    fn run(self, realm: &super::ui_realm::UiRealm) {
        match self {
            Self::Input(input) => realm.handle_input_entered(input),
            Self::Resized { size, scale_factor } => {
                // Take the applier out of the TLS slot, release the borrow,
                // call it, then restore it — never call through a live
                // borrow, so a reentrant TLS access from inside the applier
                // (e.g. a nested dispatch enqueuing further work) cannot hit
                // an already-mutably-borrowed `RefCell` panic. If the slot is
                // ever found empty here (no applier installed yet, or
                // already cleared by teardown) this skips with a trace
                // instead of unwrapping/panicking; surface application then
                // coalesces onto the next real applier install.
                let applier = APP_RUNTIME.with(|slot| slot.borrow_mut().surface_applier.take());
                match applier {
                    Some(applier) => {
                        // The guard restores the applier on drop
                        // unconditionally — including if `call` below
                        // panics and the drop runs during unwind — so a
                        // caught panic in the applier never permanently
                        // strands resizing.
                        let mut guard = SurfaceApplierRestoreGuard(Some(applier));
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
                let (old, new) = APP_RUNTIME.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let old = derive_lifecycle_state(state.visible, state.focused);
                    state.focused = focused;
                    (old, derive_lifecycle_state(state.visible, state.focused))
                });
                emit_lifecycle_transition(realm, old, new);
            }
            Self::WindowVisibility(visible) => {
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

/// Installs `applier` as the registration-lifetime renderer-surface
/// applier for the current realm host, replacing (never stacking) any
/// previously-installed one. Call once per realm install, alongside
/// [`install_platform_realm`], from each backend's bootstrap — never from
/// inside a frame/event dispatch.
#[cfg(not(target_os = "ios"))]
fn install_surface_applier(
    applier: impl FnMut(flui_types::Size<flui_types::geometry::Pixels>, f32) + 'static,
) {
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut().surface_applier = Some(Box::new(applier));
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
/// reroute if `Scheduler::lifecycle_state()`'s corrupt-byte fallback
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
/// step at a time, to both the realm's own `Scheduler` and its
/// `WidgetsBinding` observers — mirroring Flutter's single platform-message
/// stream driving both `SchedulerBinding` and `WidgetsBinding` from the same
/// synthesized sequence of states.
///
/// Installed as a direct call in the same `PlatformToUi` handler (never a
/// `Scheduler`-listener closure): a listener captured at bootstrap time
/// would have to resolve `realm`/`WidgetsBinding` lazily at fire time,
/// which is unsound here specifically because every production caller of
/// this function runs from inside `dispatch_platform_realm`'s dispatch
/// window — the window during which the realm is taken OUT of
/// `APP_RUNTIME` and only restored once the dispatched task returns. A
/// listener resolving `APP_RUNTIME` at fire time would see `None` on every
/// real transition and silently no-op (this shipped once and was caught by
/// `frames_reenable_redirties_root_when_dispatched_through_the_realm_queue`
/// in `realm_dispatch_tests`, which reproduces via a real dispatched
/// `PlatformToUi::Lifecycle` sequence rather than driving `Scheduler`
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
    /// Reachable via Android's Pause/Resume reroute if `Scheduler::
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

/// Installs `realm`, minting its dispatcher's address by registering
/// `window` in the single [`super::window_registry::WindowRegistry`]
/// authority — the registry is the sole mint path for a routable
/// [`flui_foundation::PresentationAddress`]; no caller of this function ever
/// names the platform-internal native-handle key type itself.
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
    let (displaced_realm, displaced_queue, displaced_applier) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // A realm may already be installed here — a reinstall without an
        // intervening `teardown_platform_realm` (the panic-recovery path: a
        // mid-`on_ready` failure leaves the old realm/queue/applier/registry
        // mappings in place, and bootstrap tries again on the same thread).
        // Remove every registry mapping addressed to the DISPLACED realm —
        // not just the window being installed now — in this same borrow,
        // before registering the new window: otherwise the displaced
        // realm's own window(s) survive as dead entries no later teardown
        // ever reaches (teardown only ever removes the realm that is
        // *currently* installed).
        let previous_address = state.address;
        let mut removed_window_mappings = 0;
        if let Some(previous_address) = previous_address {
            removed_window_mappings = state.registry.remove_realm(previous_address.realm_id).len();
        }
        state.registry.register_window(window, address);

        // Mirror `teardown_platform_realm`'s discipline exactly: `mem::take`
        // the displaced realm, its queue, and its surface applier out from
        // under this borrow — never let an assignment drop them while the
        // borrow is still live, and never leave stale-incarnation events
        // sitting in the queue to be delivered FIFO-first into the new
        // realm on its first dispatch.
        let displaced_realm = state.realm.take();
        let displaced_queue = std::mem::take(&mut state.queue);
        let displaced_applier = state.surface_applier.take();
        if displaced_realm.is_some() {
            tracing::warn!(
                previous_address = ?previous_address,
                new_address = ?address,
                queued_events_discarded = displaced_queue.len(),
                removed_window_mappings,
                "install_platform_realm: replacing a realm that was never torn down"
            );
        }
        state.realm = Some(realm);
        state.owner_thread = Some(owner_thread);
        state.address = Some(address);
        state.draining = false;
        // Defensive: a reinstall-without-teardown only reaches this path
        // when the displaced incarnation's own dispatch never restored
        // `realm` (the panic-recovery scenario this function's doc already
        // documents) — `dispatched_scheduler`, if the displaced incarnation
        // left one stashed, belongs to that dead incarnation and must not
        // leak into the fresh one's fence-(c) reads.
        state.dispatched_scheduler = None;
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
        (displaced_realm, displaced_queue, displaced_applier)
    });
    // Destructors may re-enter platform/framework code (the same invariant
    // `teardown_platform_realm` honors) — drop only after the TLS borrow
    // above has released.
    drop(displaced_queue);
    drop(displaced_realm);
    drop(displaced_applier);
    RealmDispatcher {
        owner_thread,
        address,
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
    let realm = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Normative compare order (ADR-0037): realm first, then
        // presentation. `realm_id`/`presentation_id` mint from one shared
        // counter, so teardown+reinstall always changes both and the realm
        // check fires first on the common path; `StalePresentation` is
        // reachable only when the realm half matches but the presentation
        // half does not (a forged/mixed address today; real presentation
        // replacement within a live realm once a forest exists).
        match state.address {
            None => {
                tracing::debug!(
                    ?dispatcher,
                    "dropping realm callback: no realm installed (not yet ready, or already torn down)"
                );
                return Err(RealmDispatchError::RealmUnavailable);
            }
            Some(current) if current.realm_id != dispatcher.address.realm_id => {
                tracing::debug!(
                    ?dispatcher,
                    current_realm_id = ?current.realm_id,
                    "dropping realm callback: a newer realm replaced the one it was dispatched for"
                );
                return Err(RealmDispatchError::StaleRealm);
            }
            Some(current) if current.presentation_id != dispatcher.address.presentation_id => {
                tracing::debug!(
                    ?dispatcher,
                    current_address = ?current,
                    "dropping realm callback: presentation incarnation mismatch within the live realm"
                );
                return Err(RealmDispatchError::StalePresentation);
            }
            Some(_) => {}
        }
        state.queue.push_back(event);
        if state.draining || state.realm.is_none() {
            return Ok(None);
        }
        let first = state
            .queue
            .pop_front()
            .expect("BUG: event was enqueued before starting realm dispatch");
        state.draining = true;
        let realm = state.realm.take();
        // Stash a clone of the checked-out realm's scheduler BEFORE it leaves
        // this slot: `installed_realm_phase` (with_owner_platform's fence
        // (c)) reads `dispatched_scheduler` as its fallback whenever `realm`
        // itself is empty, which is exactly the state this call is about to
        // create for the entire duration of the dispatched task below —
        // otherwise the fence goes blind for every real production frame,
        // not merely when no realm is installed at all. `Scheduler::clone`
        // is one `Arc::clone` (see `flui-scheduler`'s single-`Arc` handle
        // shape), not a second scheduler.
        state.dispatched_scheduler = realm.as_ref().map(|realm| realm.scheduler().clone());
        Ok(realm.map(|realm| (realm, first)))
    })?;
    let Some((realm, first)) = realm else {
        return Ok(());
    };

    // Never hold the TLS RefCell borrow across user/platform callbacks. Catch
    // only to restore the host invariants; the original panic is resumed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut next = Some(first);
        while let Some(event) = next {
            realm.enter(|realm| event.run(realm));
            next = APP_RUNTIME.with(|slot| slot.borrow_mut().queue.pop_front());
        }
    }));
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.realm = Some(realm);
        state.draining = false;
        // Cleared unconditionally in this same restore block, which runs
        // whether or not the dispatched task above panicked (the panic, if
        // any, is only resumed after this restore completes below) — the
        // fence-(c) fallback must not survive past the dispatch it was
        // stashed for.
        state.dispatched_scheduler = None;
    });
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
    Ok(())
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
    let (realm, queued) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Registry removal first (ADR-0037 §2): stop new routing before the
        // queued old-generation events below are dropped, and before the
        // realm/address cache is cleared. The teardown real read: assert
        // the removed entries include the address this realm installed —
        // `remove_realm` removes every window mapped to this realm, not
        // just the first, so a future one-realm-many-windows install still
        // leaves nothing behind.
        if let Some(address) = state.address {
            let removed = state.registry.remove_realm(address.realm_id);
            debug_assert!(
                removed
                    .iter()
                    .any(|(_, removed_address)| *removed_address == address),
                "BUG: window_registry teardown read did not include the installed address"
            );
        }
        let realm = state.realm.take();
        let queued = std::mem::take(&mut state.queue);
        state.draining = false;
        state.owner_thread = None;
        state.address = None;
        state.surface_applier = None;
        state.dispatched_scheduler = None;
        (realm, queued)
    });
    // Destructors may re-enter platform/framework code. Drop only after the
    // TLS borrow and incarnation identity have been released.
    drop(queued);
    drop(realm);

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
    use std::{cell::RefCell, rc::Rc};

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
            state.queue.push_back(RealmTask::Frame(Box::new(move |_| {
                *delivered_in_event.borrow_mut() = true;
            })));
            state
                .queue
                .push_back(RealmTask::Frame(Box::new(move |_| drop(probe))));
            state.draining = true;
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
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm = state.realm.take();
            state.queue.clear();
            state.address = None;
            drop(state);
            drop(realm);
        });
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
        let _dispatcher = install_test_realm();
        let delivered = Rc::new(RefCell::new(false));
        let delivered_in_event = Rc::clone(&delivered);
        let applier_invoked = Rc::new(RefCell::new(false));
        let applier_invoked_in_closure = Rc::clone(&applier_invoked);
        install_surface_applier(move |_size, _scale_factor| {
            *applier_invoked_in_closure.borrow_mut() = true;
        });

        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            state.queue.push_back(RealmTask::Frame(Box::new(move |_| {
                *delivered_in_event.borrow_mut() = true;
            })));
            state
                .queue
                .push_back(RealmTask::Event(PlatformToUi::Resized {
                    size: flui_types::Size::new(
                        flui_types::geometry::px(100.0),
                        flui_types::geometry::px(100.0),
                    ),
                    scale_factor: 1.0,
                }));
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
        install_surface_applier(move |_size, _scale_factor| {
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
        install_surface_applier(move |_size, _scale_factor| {
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
                .queue
                .push_back(RealmTask::Frame(Box::new(move |_| drop(probe))));
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
    /// returns. A fire-time `APP_RUNTIME` lookup (a `Scheduler` lifecycle
    /// listener, the previous shape of this fix) can never see the realm
    /// during that exact window — driving a throwaway `Scheduler` directly,
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
    /// `Detached`): poll only [`Scheduler::drive_async_tasks`](flui_scheduler::Scheduler::drive_async_tasks) — never
    /// begin/draw a frame, tick, run the pipeline, or present. Dirty work
    /// is left untouched; it accumulates until frames re-enable.
    PumpAsync,
    /// A spurious wake while frames are enabled: nothing dirty, no
    /// scheduled ticker. No render, no pump, no sleep.
    Skip,
}

/// Decides what a platform wake should do, given the scheduler's
/// [`Scheduler::frames_enabled`](flui_scheduler::Scheduler::frames_enabled) fact (ADR-0035) alongside the pre-existing
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
/// the global `Scheduler` has a pending ticker callback (a running
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
/// `Scheduler::finish_async_pump` lets keep waking the loop has no
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
    /// `Scheduler::drive_async_tasks` call while frames are disabled, with
    /// no frame ever advancing — and a `Resumed` transition afterward must
    /// produce exactly one frame.
    ///
    /// Standalone `Scheduler::new()`, not the process singleton: this test
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

        use flui_scheduler::{AppLifecycleState, Scheduler};

        let scheduler = Scheduler::new();
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
        scheduler.drive_frame(web_time::Instant::now(), || {});
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
            install_surface_applier(move |size, scale_factor| {
                let w = (size.width.0 * scale_factor) as u32;
                let h = (size.height.0 * scale_factor) as u32;
                renderer_resize.lock().resize(w, h);
            });
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
                    // `Scheduler::finish_async_pump`'s doc for the full
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

            let now = web_time::Instant::now();

        // Scheduler callbacks (animations). NOTE: the global `Scheduler` is driven
        // off this per-frame `Instant::now()`, while the tree-bound `Vsync`
        // (`UiRealm::draw_frame`) ticks off the realm's own `start` origin —
        // two separate clocks ON PURPOSE: the controller sets are disjoint (implicit
        // animations register with `Vsync`; plain controllers carry a private
        // `Scheduler` ticker, never the global one), so the origins never need to
        // agree and no controller is advanced twice.
        // The ONE shared frame ordering — begin (transient +
        // microtasks + the single async-driver poll) -> persistent callbacks ->
        // the pipeline below -> post-frame callbacks -> Idle. `HeadlessBinding`
        // drives the same helper on its binding-local scheduler.
            let presented = scheduler.drive_frame(now, || {
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
            });

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
                    // scheduler left to notify as a fallback -- unlike the
                    // singleton era, there is genuinely no observer left.
                    tracing::warn!(
                        ?error,
                        "realm unavailable during Detached lifecycle dispatch"
                    );
                }
            }));
        });

        // Window close -> log and let the platform handle quit
        // (Windows window proc already calls PostQuitMessage on WM_DESTROY)
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
        }));

        // Window should-close -> allow by default
        window.on_should_close(Box::new(|| {
            tracing::debug!("Window close requested, allowing");
            true
        }));

        // Window focus/visibility -> the `(visible, focused)`
        // `AppLifecycleState` derivation. `on_visibility_status_change`
        // rides winit's `Occluded` event; Wayland delivery is
        // compositor-conditional (see that callback's doc) — where a
        // compositor never sends it, the window is treated as always
        // visible (the same as before this callback existed).
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
            install_surface_applier(move |size, scale_factor| {
                let w = (size.width.0 * scale_factor) as u32;
                let h = (size.height.0 * scale_factor) as u32;
                renderer_resize.lock().resize(w, h);
            });
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
                            // see `Scheduler::finish_async_pump`'s doc for the
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
                    // Scheduler callbacks and rendering share ONE `UiRealm::enter`
                    // dynamic extent; callbacks may legally resolve realm-local
                    // capabilities throughout the complete frame transaction.
                    scheduler.drive_frame(now, || {
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
                    });
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

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");

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
            install_surface_applier(move |size, scale_factor| {
                if let Some(renderer) = renderer_resize.lock().as_mut() {
                    let width = (size.width.0 * scale_factor) as u32;
                    let height = (size.height.0 * scale_factor) as u32;
                    renderer.resize(width, height);
                }
            });
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
                            // see `Scheduler::finish_async_pump`'s doc for the
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
                    // Scheduler callbacks and rendering share one realm entry.
                    scheduler.drive_frame(now, || {
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
                    });
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
    /// window-identity lock, and later, briefly, `Scheduler` carried a
    /// sibling per-test scheduler-phase lock; both are deleted now, not
    /// ported forward, because the state each one guarded
    /// (`AppBinding::instance()`'s active window, and the process-global
    /// half of the `Scheduler` singleton respectively) no longer exists —
    /// `AppBinding` is gone entirely and every `UiRealm` owns its own fresh
    /// `Scheduler` value — and because a per-test-thread thread-local needs
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
        install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);

        // A clone of the installed realm's OWN scheduler -- same underlying
        // `SchedulerInner` fence (c) reads through `installed_realm_phase`.
        // Driven directly here, with the realm still resident in `realm`
        // (not checked out via `dispatch_platform_realm`) -- the sibling
        // test right below this one, `..._through_dispatch`, pins the other
        // half: the realm checked OUT for a dispatched task, where
        // `installed_realm_phase` must fall back to `dispatched_scheduler`
        // instead of reading `realm` directly.
        let scheduler = APP_RUNTIME.with(|slot| {
            slot.borrow()
                .realm
                .as_ref()
                .expect("just installed above")
                .scheduler()
                .clone()
        });

        // `drive_frame` leaves the scheduler in `PersistentCallbacks` for the
        // duration of its `pipeline` closure -- a forbidden phase per fence
        // (c). A panicking pipeline is caught internally and resolved back
        // to `Idle` via `abort_frame()` before the panic resumes, so this
        // test's own `#[should_panic]` unwind leaves the scheduler clean.
        scheduler.drive_frame(web_time::Instant::now(), || {
            let _ = with_owner_platform(|_owner| ());
        });
    }

    /// The through-dispatch half of the fence-(c) pin above: `with_owner_platform`
    /// called from INSIDE a `RealmTask::Frame` running through
    /// `dispatch_platform_realm` -- not driven directly against a resident
    /// realm -- must still trip the debug_assert while a frame phase is
    /// active.
    ///
    /// Red before the `dispatched_scheduler` fallback existed:
    /// `dispatch_platform_realm` checks the realm OUT of `AppRuntime.realm`
    /// for the entire extent of the dispatched task (see its own doc), so
    /// `installed_realm_phase` reading only `realm` would observe `None` for
    /// this call, not `PersistentCallbacks` -- vacuously "not mid-frame",
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
        // the realm is checked out of `AppRuntime.realm` for the whole
        // closure below, exactly the window `dispatched_scheduler` exists to
        // cover. `drive_frame`'s `PersistentCallbacks` phase is active while
        // `with_owner_platform` is called, so the fence must trip here
        // exactly as it does when driven directly.
        let dispatch_result = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                realm.scheduler().drive_frame(web_time::Instant::now(), || {
                    let _ = with_owner_platform(|_owner| ());
                });
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
