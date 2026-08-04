//! `AppRuntime` — the loop-scoped composition root.
//!
//! # Thesis
//!
//! This module creates the composition root and the constructor-injection
//! seams while every service it names is still singleton-*backed*. The
//! honest claim: `UiRealm` performs zero `::instance()` calls; the services
//! it consumes are resolved once, here, in [`SharedEngineServices::resolve`].
//! Other ambient reaches (`renderer_binding.rs`, `binding.rs`,
//! `hot_reload.rs`, `config.rs`) are untouched — they remain until the
//! change that retires each singleton they reach for.
//! `flui-engine/src/wgpu/text.rs`'s ambient reach for painting has since
//! closed: `TextRenderer::new` takes an injected `SharedFontSystem`
//! parameter instead of calling `PaintingBinding::instance()` itself. This
//! is not a forwarding shim: no old API is preserved-but-deprecated here,
//! and no ambient access point this change does not touch is claimed as
//! closed.
//!
//! # What lives here vs. in `runner.rs`
//!
//! `AppRuntime` absorbs the transitional `RealmHost`'s fields (realm slot,
//! queue, draining flag, owner thread, address cache, window registry,
//! surface applier, visible/focused) plus the loop-scoped
//! `OwnerPlatform` capability (formerly a second, separate thread-local) and
//! [`SharedEngineServices`]. The single-threaded dispatch machinery that
//! operates on this struct — `install_platform_realm`,
//! `dispatch_platform_realm`, `teardown_platform_realm`,
//! `install_owner_platform`, `with_owner_platform`, the TLS declaration
//! itself — stays in `runner.rs`, unchanged in behavior: this change moves
//! *ownership* (one struct, one thread-local slot instead of two), not the
//! dispatch/teardown semantics those functions implement.

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_foundation::{HasInstance, PresentationId, RealmId};
use flui_scheduler::{AsyncDriver, LocalPostFrameLane, Scheduler, SchedulerPhase};

// `SchedulerRef`, `RealmServices`, and `next_identity` below are used
// unconditionally by `ui_realm.rs` (every `UiRealm` constructor resolves its
// own `RealmServices` now, on every platform `UiRealm` itself compiles for,
// including iOS's stub). `AppRuntime` and `SharedEngineServices` further
// down are the loop-scoped composition root that only the non-iOS runners
// (`runner.rs`'s desktop/android/web dispatch) instantiate, so they -- and
// the imports only they need -- stay `#[cfg(not(target_os = "ios"))]`,
// matching the cfg the absorbed `RealmHost`/`OWNER_PLATFORM_HOST` carried.

/// Newtype over `&'static Scheduler`, resolved once in
/// [`SharedEngineServices::resolve`] / [`RealmServices::resolve`].
///
/// Exists rather than a bare `&'static Scheduler` field so a future flip to
/// a realm-owned `Scheduler` value changes one type's internals, not every
/// call site's field-access syntax — the same "flip-containment" shape
/// `PaintingBinding::font_system` already proves for painting (it returns
/// `SharedFontSystem` by value, so consumers never observe the `'static`
/// lifetime).
///
/// # Thread affinity
///
/// `Scheduler::instance()` is itself thread-local — `impl_binding_singleton!`
/// `Box::leak`s a *separate* instance per owner thread, so the `&'static`
/// inside this newtype is only meaningful on the thread that resolved it. A
/// bare `&'static Scheduler` field would make this struct auto-`Send +
/// Sync` (a `&'static T` is `Send + Sync` whenever `T: Sync`, regardless of
/// which thread produced the reference), which would let a `SchedulerRef`
/// minted on thread A cross to thread B and silently read thread A's phase
/// there instead of failing to compile or panicking. That is latent today
/// only because every current holder (`UiRealm`, `AppRuntime`) is itself
/// already `!Send` for unrelated reasons — it turns actively wrong the
/// moment either type's ownership model changes to allow crossing threads
/// while still carrying a `SchedulerRef`. The `PhantomData<*const ()>`
/// field below is the same zero-cost thread-affinity marker `UiRealm` uses
/// for itself (`_owner_affine`), and the item-position assertion after this
/// impl block pins `!Send + !Sync` as a compile-time fact, not a comment.
#[derive(Clone, Copy)]
pub(crate) struct SchedulerRef {
    scheduler: &'static Scheduler,
    _owner_affine: PhantomData<*const ()>,
}

impl SchedulerRef {
    /// Mints a fresh handle from the current owner thread's `Scheduler`
    /// singleton.
    ///
    /// `pub(crate)` rather than module-private: [`SharedEngineServices`] and
    /// [`RealmServices`] below are each `resolve()`'s primary, cached
    /// caller, but this is also the ONE typed reach a same-thread ambient
    /// `Scheduler::instance()` call site elsewhere in this crate
    /// (`bindings/renderer_binding.rs`) is rewritten onto, instead of
    /// touching the raw singleton accessor directly — the newtype's
    /// thread-affinity guarantee (see the type-level doc above) applies
    /// equally whether the handle is cached or resolved fresh per call;
    /// `Scheduler::instance()` itself already memoizes per owner thread, so
    /// resolving here instead of caching costs nothing beyond the wrapper.
    pub(crate) fn resolve() -> Self {
        Self {
            scheduler: Scheduler::instance(),
            _owner_affine: PhantomData,
        }
    }

    /// The current scheduler phase — the seam `UiRealm::drain_commands`
    /// reads for its idle-only commit-gate debug assertion, so `UiRealm`
    /// itself never has to call `Scheduler::instance()`.
    pub(crate) fn phase(&self) -> SchedulerPhase {
        self.scheduler.phase()
    }

    /// Borrow the backing scheduler directly, for the handful of
    /// construction-time calls ([`RealmServices::resolve`]) that need more
    /// than the phase probe. This is the one place that leaks the
    /// `&'static` the newtype otherwise exists to contain -- it dies with
    /// the scheduler-ownership flip: once `Scheduler` becomes an owned,
    /// realm-local value instead of a `'static` singleton, this method
    /// simply stops type-checking, which is the point.
    pub(crate) fn get(&self) -> &'static Scheduler {
        self.scheduler
    }
}

#[cfg(test)]
mod scheduler_ref_tests {
    use super::*;

    // Compile-time fence: a `SchedulerRef` minted on one thread must never
    // be movable/shareable to another, because the `&'static Scheduler` it
    // wraps is only meaningful on the thread that resolved it
    // (`Scheduler::instance()` is thread-local underneath). Widening this
    // bound is exactly the landmine described on `SchedulerRef`'s own
    // rustdoc -- it would silently compile a cross-thread phase read.
    static_assertions::assert_not_impl_any!(SchedulerRef: Send, Sync);
}

#[cfg(not(target_os = "ios"))]
use std::cell::OnceCell;
#[cfg(not(target_os = "ios"))]
use std::collections::VecDeque;
#[cfg(not(target_os = "ios"))]
use std::sync::Arc;
#[cfg(not(target_os = "ios"))]
use std::sync::atomic::AtomicBool;
#[cfg(not(target_os = "ios"))]
use std::thread::ThreadId;

#[cfg(not(target_os = "ios"))]
use flui_foundation::PresentationAddress;
#[cfg(not(target_os = "ios"))]
use flui_painting::PaintingBinding;
#[cfg(not(target_os = "ios"))]
use flui_platform::OwnerPlatform;
#[cfg(not(target_os = "ios"))]
use flui_platform::traits::{Clipboard, PlatformWindow};
#[cfg(not(target_os = "ios"))]
use flui_semantics::AccessibilityFeatures;
#[cfg(not(target_os = "ios"))]
use parking_lot::{Mutex, RwLock};

#[cfg(not(target_os = "ios"))]
use super::runner::{RealmTask, SurfaceApplier};
#[cfg(not(target_os = "ios"))]
use super::ui_realm::UiRealm;
#[cfg(not(target_os = "ios"))]
use super::window_registry::WindowRegistry;

/// Process-level engine services, each resolved **once** per owner thread in
/// [`SharedEngineServices::resolve`] — never re-resolved on every access, and
/// never reached ambiently from inside `UiRealm`.
///
/// `painting` is an OWNED [`PaintingBinding`] value: `PaintingBinding`'s
/// singleton-macro invocation was deleted as the remaining singletons here
/// move behind explicit owners one at a time, so this is the first field
/// here to complete its flip from a `'static` singleton reference to a
/// plain owned value — the same "flip-containment" shape
/// `PaintingBinding::font_system` already proved possible (it returns
/// `SharedFontSystem` by value, so consumers never observed the `'static`
/// lifetime in the first place). `accessibility_features` completed the
/// same flip alongside `painting`: the retired `SemanticsBinding` singleton
/// no longer exists at all (its enablement/announce/event state moved to
/// the per-presentation `SemanticsHost` instead — see
/// `super::semantics_host` — since that half of the old binding was a
/// per-window platform seam, not process-global state); only the OS-level,
/// read-mostly accessibility flags stayed process-scoped, and this struct
/// now owns that value directly. `scheduler` is still the process-global
/// singleton underneath (`Scheduler::instance()`) — only the *resolution
/// point* moves to this one constructor; it flips to an owned value once
/// the change that retires the scheduler singleton lands. Until then this
/// field is `pub(super)`, not part of any public surface (ADR-0027 §9:
/// transitional runtime types stay `pub(crate)` at most).
#[cfg(not(target_os = "ios"))]
pub(crate) struct SharedEngineServices {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
        )
    )]
    pub(super) painting: PaintingBinding,
    /// OS-level accessibility flags (reduced motion, high contrast, ...).
    /// Process-scoped and read-mostly — re-homed here from the retired
    /// `SemanticsBinding` singleton (see this struct's own doc comment).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
        )
    )]
    pub(super) accessibility_features: RwLock<AccessibilityFeatures>,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
        )
    )]
    pub(super) scheduler: SchedulerRef,
}

#[cfg(not(target_os = "ios"))]
impl SharedEngineServices {
    /// Current accessibility features (by value — mirrors the retired
    /// `SemanticsBinding::accessibility_features` accessor's shape).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
        )
    )]
    pub(super) fn accessibility_features(&self) -> AccessibilityFeatures {
        *self.accessibility_features.read()
    }

    /// Updates the accessibility features, typically called by the
    /// platform embedder when OS accessibility settings change.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
        )
    )]
    pub(super) fn set_accessibility_features(&self, features: AccessibilityFeatures) {
        *self.accessibility_features.write() = features;
    }

    /// `wake` is `AppRuntime::frame_wake_callback()` — the same `Send +
    /// Sync` capability [`AppRuntime::ensure_services`] passes in, extracted
    /// by its caller BEFORE the `&mut self.services` borrow that resolves
    /// this struct, so both can happen in one non-conflicting call.
    fn resolve(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let painting = PaintingBinding::new();

        // `SharedEngineServices::resolve()` -- reached only through
        // `AppRuntime::ensure_services()`, at the realm-install point -- is
        // the CONSTRUCTING owner of the free-standing `FONT_SYSTEM`
        // `OnceLock` slot (`flui-painting/src/text_layout/layout.rs`):
        // initialize it explicitly, here, at a known point, rather than
        // leaving it to whichever text-measurement call happens to run
        // first on this thread. The read path stays ambient on layout hot
        // paths: this is a named exclusion (see this crate's
        // `ambient_reach` entry in `docs/runtime-contract.toml`), not closed
        // here -- injecting the font system into every `perform_layout`
        // text-measurement call is a separate, larger follow-up (the
        // icon-font-loading/sharding work).
        let _ = painting.font_system();

        let scheduler = SchedulerRef::resolve();

        // Once-per-thread wiring, moved here from the retired
        // `AppBinding::instance()`'s TLS initializer. `ensure_services`
        // resolving THIS struct exactly once per owner thread
        // (`OnceCell::get_or_init`) is what makes this installation
        // steal-proof and idempotent — the same guarantee the retired
        // `instance()`'s thread-local initializer gave: a throwaway
        // `UiRealm::for_test()` built alongside a live `AppRuntime` never
        // touches this hook (it never resolves `SharedEngineServices` at
        // all), and a second realm installed on this same thread
        // (hot-restart) never re-registers it either.
        //
        // Animation-wake wiring: scheduling a frame callback (a ticker tick
        // on the `Scheduler::instance()` singleton, e.g. an
        // `AnimationController` built directly against it rather than a
        // realm's Vsync registry) fires this hook on the scheduler's
        // false->true `frame_scheduled` transition. The SAME hook also
        // fires from the async-driver's task waker whenever a spawned
        // future's `Waker::wake` runs, possibly from a thread that never
        // touched this runtime at all -- `wake` is exactly the `Arc`-backed,
        // `Send + Sync` handle that makes firing it safe regardless of
        // which thread does so.
        //
        // The frames-disabled->enabled root re-dirty (an app that was
        // `Hidden`/`Paused`/`Detached` coming back to `Resumed`/`Inactive`
        // needs its root explicitly re-dirtied, or the next frame finds
        // nothing dirty and stays Idle) does NOT live here as a `Scheduler`
        // lifecycle listener. It used to, and that shipped a real bug: every
        // production lifecycle transition runs through
        // `runner.rs`'s `emit_lifecycle_transition`, which itself always
        // runs inside `dispatch_platform_realm`'s dispatch window — the
        // window during which the realm is taken OUT of `APP_RUNTIME` and
        // only restored once the dispatch returns. A listener resolving
        // `APP_RUNTIME` at fire time therefore always saw `None` and
        // silently no-opped on every real transition. The fix lives at
        // `emit_lifecycle_transition` itself, which already has the live
        // realm in scope and needs no thread-local resolution at all — see
        // that function's own doc.
        scheduler
            .get()
            .set_on_frame_scheduled(Some(Arc::clone(&wake)));

        Self {
            painting,
            accessibility_features: RwLock::new(AccessibilityFeatures::default()),
            scheduler,
        }
    }
}

/// What [`UiRealm::construct`](super::ui_realm) needs from the scheduler at
/// construction time: `local_post_frame_lane()` and `async_driver()`
/// (formerly `Scheduler::instance()` calls inside `ui_realm.rs` itself),
/// plus the [`SchedulerRef`] `UiRealm` retains for its idle-only commit-gate
/// phase probe (formerly another `Scheduler::instance()` call in the same
/// file). Resolved once, here — never inside `ui_realm.rs` itself, so
/// `UiRealm`'s own source performs zero `::instance()` calls.
pub(crate) struct RealmServices {
    pub(crate) local_post_frame: LocalPostFrameLane,
    pub(crate) async_driver: AsyncDriver,
    pub(crate) scheduler: SchedulerRef,
}

impl RealmServices {
    /// Resolves directly from the still-singleton-backed `Scheduler`.
    /// Because `Scheduler::instance()` memoizes per owner thread, this
    /// yields the exact same `&'static Scheduler` a live `AppRuntime`'s own
    /// [`SharedEngineServices`] resolved on this thread — whether or not an
    /// `AppRuntime` has actually been touched yet. That is what lets every
    /// `UiRealm` constructor (`new`, `with_capacity`, `for_test`,
    /// `for_test_with_text_input`) call this instead of reaching for
    /// `Scheduler::instance()` directly, none of them taking a process-host
    /// parameter any more (the retired `AppBinding` is gone).
    pub(crate) fn resolve() -> Self {
        let scheduler = SchedulerRef::resolve();
        let backing = scheduler.get();
        Self {
            local_post_frame: backing.local_post_frame_lane(),
            async_driver: backing.async_driver().clone(),
            scheduler,
        }
    }
}

/// Monotonic incarnation counter: every successfully constructed realm gets
/// a fresh `RealmId` generation, so a recreated realm never compares equal
/// to its predecessor. Moved here from `ui_realm.rs`: identity minting is an
/// `AppRuntime` concern now, not a `UiRealm` one — a real multi-window
/// `AppRuntime` registry mints slots from here once it exists.
static NEXT_INCARNATION: AtomicU32 = AtomicU32::new(1);

/// Mints a fresh, process-unique `(RealmId, PresentationId)` pair. Slot 0 is
/// the single-window slot; a real multi-window `AppRuntime` registry mints
/// slots once the element forest lets a realm host multiple presentations —
/// the shape is the deliverable now, single-window the only instantiation.
pub(crate) fn next_identity() -> (RealmId, PresentationId) {
    let incarnation = NEXT_INCARNATION.fetch_add(1, Ordering::Relaxed);
    let generation = NonZeroU32::new(incarnation)
        .expect("BUG: incarnation counter starts at 1 and only increments");
    (
        RealmId::new_gen(0, generation),
        PresentationId::new_gen(0, generation),
    )
}

/// Cross-thread wake capability for the platform event loop.
///
/// Setting `needs_redraw` and poking a live window are two independent
/// effects `AppRuntime::wake_frame` used to perform directly against its own
/// fields; this handle exists so the SAME two effects can be captured into a
/// `Send + Sync` closure — the scheduler's `on_frame_scheduled` hook and the
/// async-driver's task waker each fire from possibly-any thread (an executor
/// thread completing an image-decode future, for instance), never
/// necessarily the owner thread that hosts `AppRuntime`'s thread-local slot.
/// A hook that re-resolved `APP_RUNTIME` at fire time would, on a non-owner
/// thread, either find no runtime at all or (worse, on a future multi-runtime
/// host) find the WRONG one — this handle's `Arc` clones sidestep thread-local
/// resolution entirely, so firing it only ever touches shared, thread-safe
/// state and always reaches the intended owner.
#[cfg(not(target_os = "ios"))]
#[derive(Clone)]
struct FrameWakeHandle {
    needs_redraw: Arc<AtomicBool>,
    redraw_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>>,
}

#[cfg(not(target_os = "ios"))]
impl FrameWakeHandle {
    fn wake_frame(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
        let window = self.redraw_window.lock().as_ref().cloned();
        if let Some(window) = window {
            window.request_redraw();
            tracing::trace!("wake_frame: platform window request_redraw sent");
        }
    }

    fn into_callback(self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(move || self.wake_frame())
    }
}

/// The loop-scoped composition root: platform event-loop demux, the single
/// realm slot, and the once-resolved [`SharedEngineServices`].
///
/// Absorbs the former `RealmHost` (realm slot, queue, draining flag, owner
/// thread, address cache, window registry, surface applier, visible/focused)
/// wholesale, plus the loop-scoped `OwnerPlatform` capability (formerly the
/// separate `OWNER_PLATFORM_HOST` thread-local) and `services`. One struct,
/// one thread-local slot (`runner.rs`'s `APP_RUNTIME`) — the same two
/// invariants that justified two separate TLS cells before still hold as two
/// fields on one struct: `teardown_platform_realm` clears the realm-facing
/// fields and *never* `owner_platform` (hot-restart hosts a fresh realm on
/// the same loop); `OwnerHostClearGuard` clears only `owner_platform` on
/// unwind.
///
/// # Design-for-N
///
/// The realm-facing API is `RealmId`-keyed ([`AppRuntime::realm`]);
/// *storage* is a single `Option<UiRealm>` slot until the element forest
/// lets a realm host multiple presentations and this grows a real
/// multi-realm registry. `next_identity` above already mints from a shape
/// that does not need to change when that lands.
#[cfg(not(target_os = "ios"))]
pub(crate) struct AppRuntime {
    /// The single hosted realm, when one is installed.
    pub(super) realm: Option<UiRealm>,
    /// Queued owner-thread work: cross-thread platform events plus the
    /// co-located frame pump (see `runner.rs`'s `RealmTask`).
    pub(super) queue: VecDeque<RealmTask>,
    /// Set while a queued task is running, so a reentrant dispatch enqueues
    /// instead of recursing into `realm.take()`.
    pub(super) draining: bool,
    /// The thread that installed the current realm; every dispatch checks
    /// against this before touching the slot.
    pub(super) owner_thread: Option<ThreadId>,
    /// This realm incarnation's routable address — a derived cache of
    /// `registry` below, not a second source of truth (see
    /// `window_registry`'s module doc for the write-ordering invariant).
    pub(super) address: Option<PresentationAddress>,
    /// The sole native-window-to-presentation mapping authority (ADR-0037
    /// §2). A future multi-window `AppRuntime` lifts this unchanged — the
    /// type itself has no TLS assumption baked in.
    pub(super) registry: WindowRegistry,
    /// The registration-lifetime renderer-surface applier for a `Resized`
    /// event; installed once at realm install, cleared at teardown.
    pub(super) surface_applier: Option<SurfaceApplier>,
    /// Single-window `(visible, focused)` tracking for the
    /// `AppLifecycleState` derivation (ADR-0035). Both default `true`.
    pub(super) visible: bool,
    pub(super) focused: bool,
    /// The loop-scoped owner-thread platform capability (ADR-0039 §6).
    /// Deliberately *not* cleared by realm teardown — the loop may host
    /// another realm before it exits (hot-restart does exactly this).
    pub(super) owner_platform: Option<OwnerPlatform>,
    /// Process-level engine services. Deliberately **not** resolved in
    /// [`AppRuntime::new`] -- see [`AppRuntime::ensure_services`] for why.
    services: OnceCell<SharedEngineServices>,
    /// Whether a redraw has been requested since the last
    /// [`Self::mark_rendered`] — the loop-scoped half of the retired
    /// `AppBinding.needs_redraw` flag, re-homed here as part of `AppBinding`'s
    /// dissolution. Loop-scoped, not realm-scoped: a hot-restart that tears
    /// down and reinstalls a realm on this same thread must not lose a
    /// pending redraw request, and [`Self::frame_wake_callback`] hands a
    /// clone of this exact `Arc` to callbacks that may fire from any thread.
    needs_redraw: Arc<AtomicBool>,
    /// The window [`Self::wake_frame`] pokes via
    /// [`PlatformWindow::request_redraw`], installed by
    /// [`Self::set_redraw_window`] once the realm's window is open and
    /// cleared at teardown. Distinct from `PresentationState.window`
    /// (per-presentation, `Weak`, used for cursor/haptics/close): this slot
    /// is `Arc`-strong and `Send + Sync` specifically so
    /// [`Self::frame_wake_callback`] can hand a cross-thread-safe clone to a
    /// callback that fires off the owner thread, which a `Weak` field owned
    /// by a `!Send` `UiRealm` cannot support.
    redraw_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>>,
    /// The platform's clipboard capability, moved here from the retired
    /// `AppBinding` — a process/loop-scoped OS-session capability,
    /// vended to presentations rather than owned by one. `set_platform_clipboard`/
    /// `clear_platform_clipboard` are the install/teardown symmetry: without
    /// the clear half, a live platform resource (arboard on X11 owns a live
    /// X11 connection) would stay pinned behind this `Arc` past the event
    /// loop's exit. See [`Drop`]'s impl below for the last-resort third clear
    /// path.
    platform_clipboard: Arc<Mutex<Option<Arc<dyn Clipboard>>>>,
}

#[cfg(not(target_os = "ios"))]
impl AppRuntime {
    /// Construct the composition root: cheap, side-effect-free.
    /// Called as the `APP_RUNTIME` TLS slot's own initializer (`runner.rs`),
    /// so simply *touching* the thread-local -- for any reason, on any
    /// thread -- can never itself run singleton construction or full
    /// system-font enumeration. Real service resolution happens only via the
    /// explicit [`Self::ensure_services`] call from `install_platform_realm`
    /// -- when a realm is actually installed -- never from an incidental
    /// first touch such as `OwnerHostClearGuard::drop` unwinding through a
    /// virgin thread, and never from `install_owner_platform` either (every
    /// backend calls that, including `run_direct`, which never installs a
    /// realm and never needs these services).
    ///
    /// Not `const fn`: the wake/clipboard fields below need their own
    /// independent `Arc` allocations (three small ones), which the allocator
    /// makes non-const-evaluable. That is a cheap, ordinary heap allocation,
    /// not the singleton construction or system-font enumeration this
    /// function's side-effect-free contract is actually about — `services`
    /// (the `OnceCell`) staying unresolved is what that contract depends on,
    /// and this change does not touch it.
    pub(super) fn new() -> Self {
        Self {
            realm: None,
            queue: VecDeque::new(),
            draining: false,
            owner_thread: None,
            address: None,
            registry: WindowRegistry::new(),
            surface_applier: None,
            visible: true,
            focused: true,
            owner_platform: None,
            services: OnceCell::new(),
            needs_redraw: Arc::new(AtomicBool::new(false)),
            redraw_window: Arc::new(Mutex::new(None)),
            platform_clipboard: Arc::new(Mutex::new(None)),
        }
    }

    /// Resolves and caches [`SharedEngineServices`] on first call; returns
    /// the cached value on every later call. Called only from
    /// `install_platform_realm`, when a realm is actually about to be
    /// installed on this thread -- `install_owner_platform` deliberately
    /// does NOT call this (see its own doc): every backend calls that,
    /// including `run_direct`, which opens a window but never installs a
    /// realm and never consumes painting/semantics/scheduler services, so
    /// resolving there would pay for singleton construction and full
    /// system-font enumeration for nothing.
    ///
    /// This is the fix for a real hazard the previous shape had: when
    /// `AppRuntime::new()` itself resolved `SharedEngineServices` (singleton
    /// construction plus eager system-font enumeration), *any* first touch
    /// of the TLS slot ran that work -- including
    /// `OwnerHostClearGuard::drop` firing during an unwind on a thread that
    /// never got past platform init. A panic inside that resolution path
    /// would then be a second panic during an unwind already in progress,
    /// i.e. an abort that masks the original panic. Making `AppRuntime::new`
    /// infallible/side-effect-free and resolving services only from this
    /// explicit call restores the old `RealmHost`-era guarantee that merely
    /// touching the thread-local is always safe to do from within a
    /// clear-guard drop.
    pub(super) fn ensure_services(&mut self) -> &SharedEngineServices {
        let wake = self.frame_wake_callback();
        self.services
            .get_or_init(|| SharedEngineServices::resolve(wake))
    }

    /// Test-only introspection: whether `SharedEngineServices` has been
    /// resolved yet. `services` itself is private to this module, and
    /// `runner.rs`'s tests (a sibling module tree, with the real
    /// `install_owner_platform`/`install_platform_realm` bootstrap) need to
    /// assert on it without reaching into a private field directly.
    #[cfg(test)]
    pub(super) fn services_resolved(&self) -> bool {
        self.services.get().is_some()
    }

    /// `RealmId`-keyed lookup: `Some` only when `id` matches the single
    /// installed realm's identity. Storage is one slot until the element
    /// forest lets a realm host multiple presentations and `AppRuntime`
    /// grows a real multi-realm registry — the keyed shape is the
    /// deliverable now, so that growth does not have to reshape this
    /// method's callers, only its body.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "design-for-N accessor; the single production caller \
                      (per-realm dispatch) still addresses the TLS slot \
                      directly until the element forest lets a realm host \
                      multiple presentations and this grows a real \
                      multi-realm registry"
        )
    )]
    pub(crate) fn realm(&self, id: RealmId) -> Option<&UiRealm> {
        self.realm.as_ref().filter(|realm| realm.realm_id() == id)
    }

    // ========================================================================
    // Frame wake (retired from `AppBinding`)
    // ========================================================================

    fn wake_handle(&self) -> FrameWakeHandle {
        FrameWakeHandle {
            needs_redraw: Arc::clone(&self.needs_redraw),
            redraw_window: Arc::clone(&self.redraw_window),
        }
    }

    /// A `Send + Sync`, `'static` capability that sets `needs_redraw` and
    /// pokes the installed window — safe to hand to a `Scheduler` lifecycle
    /// hook, an `on_frame_scheduled` hook, or a spawned future's `Waker`,
    /// none of which may resolve this thread-local `AppRuntime` at fire time
    /// (see [`FrameWakeHandle`]'s doc).
    pub(super) fn frame_wake_callback(&self) -> Arc<dyn Fn() + Send + Sync> {
        self.wake_handle().into_callback()
    }

    /// Wake the platform event loop so the next frame is rendered: sets
    /// `needs_redraw` and, if a window is installed, calls
    /// `PlatformWindow::request_redraw()` so a quiescent event loop wakes up.
    ///
    /// Every production bootstrap wires and calls this indirectly through
    /// [`Self::frame_wake_callback`]'s `Send + Sync` closure instead of this
    /// direct form (the closure is what a `UiRealm`/cross-thread hook needs
    /// to capture); this method stays a direct, same-thread convenience —
    /// exercised today by this module's own tests and the ordering proof in
    /// `runner.rs`'s `desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production call sites go through frame_wake_callback()'s \
                      Send + Sync closure instead of this direct, same-thread form"
        )
    )]
    pub(super) fn wake_frame(&self) {
        self.wake_handle().wake_frame();
    }

    /// Request a redraw without poking the window — the flag-only half of
    /// [`Self::wake_frame`], for callers already inside a live dispatch that
    /// does not need to wake an idle loop (mirrors the retired
    /// `AppBinding::request_redraw`).
    #[expect(
        dead_code,
        reason = "production redraw requests are realm-scoped \
                  (UiRealm::request_redraw, sharing this same needs_redraw \
                  atomic via needs_redraw_handle); no loop-scoped caller \
                  needs the direct form yet, and no test exercises the \
                  flag-only form in isolation from wake_frame either"
    )]
    pub(super) fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Whether a redraw is needed.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production reads go through UiRealm::needs_redraw, sharing \
                      this same needs_redraw atomic via needs_redraw_handle"
        )
    )]
    pub(super) fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark the frame as rendered, clearing the redraw flag.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production clears go through UiRealm::mark_rendered, sharing \
                      this same needs_redraw atomic via needs_redraw_handle"
        )
    )]
    pub(super) fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// A clone of the `needs_redraw` flag, for a `UiRealm`'s own
    /// flag-only redraw requests (its `attach_root_widget`/
    /// `handle_input_entered` call sites) — the SAME atomic this runtime
    /// reads, so either side observes the other's writes.
    pub(super) fn needs_redraw_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.needs_redraw)
    }

    /// Install the window [`Self::wake_frame`] pokes. Called once the
    /// realm's window is open, before anything that could synchronously
    /// observe it (the initial redraw request, `Lifecycle::Started`) runs —
    /// otherwise the first such observer would silently see no window.
    pub(super) fn set_redraw_window(&self, window: Arc<dyn PlatformWindow>) {
        *self.redraw_window.lock() = Some(window);
    }

    /// Test-only: read-only access to the installed redraw-poke window,
    /// without going through [`Self::wake_frame`]'s poke — lets a test
    /// observe whether `set_redraw_window` took effect before some other
    /// action runs, the same ordering proof the retired
    /// `AppBinding::with_window` supported.
    #[cfg(test)]
    pub(super) fn with_redraw_window<R>(
        &self,
        f: impl FnOnce(&dyn PlatformWindow) -> R,
    ) -> Option<R> {
        self.redraw_window.lock().as_ref().map(|w| f(w.as_ref()))
    }

    /// Remove the installed redraw-poke window at teardown, so a torn-down
    /// realm's window is not kept artificially alive by this slot.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "only teardown_platform_realm calls this, and that function \
                      is desktop/android-only (the web backend's host stays \
                      resident for the page's lifetime)"
        )
    )]
    pub(super) fn clear_redraw_window(&self) {
        self.redraw_window.lock().take();
    }

    // ========================================================================
    // Clipboard, moved from the retired `AppBinding`
    // ========================================================================

    /// Install the platform's clipboard capability. See `AppBinding`'s
    /// former doc (now this field's) for why this is a plain slot rather
    /// than a new `Platform` surface.
    pub(super) fn set_platform_clipboard(&self, clipboard: Arc<dyn Clipboard>) {
        *self.platform_clipboard.lock() = Some(clipboard);
    }

    /// The explicit, deterministic teardown clear — the first of the two
    /// non-last-resort clear paths (the second is the bootstrap `set` itself
    /// replacing a prior installation; see [`Drop`]'s impl for the third,
    /// last-resort path).
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "only teardown_platform_realm calls this, and that function \
                      is desktop/android-only (the web backend's host stays \
                      resident for the page's lifetime)"
        )
    )]
    pub(super) fn clear_platform_clipboard(&self) {
        self.platform_clipboard.lock().take();
    }

    /// Access the installed platform clipboard, if any.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- a Clipboard capability \
                      through BuildContext is future wiring; kept for parity \
                      with the retired AppBinding::clipboard accessor"
        )
    )]
    pub(super) fn clipboard(&self) -> Option<Arc<dyn Clipboard>> {
        let clipboard = self.platform_clipboard.lock().clone();
        if clipboard.is_none() {
            tracing::debug!(
                "AppRuntime::clipboard: no platform clipboard installed (not yet \
                 bootstrapped, or torn down)"
            );
        }
        clipboard
    }

    /// Test-only: a clone of the exact `Arc<Mutex<...>>` slot [`Self::clipboard`]
    /// reads from. A fake `Clipboard` stored in this slot must be `'static +
    /// Send + Sync`, so it cannot safely borrow the owner `AppRuntime` back
    /// through the slot it occupies; cloning the slot instead lets a test
    /// reproduce [`Self::clipboard`]'s exact lock-then-clone-then-drop
    /// sequence without a self-reference.
    #[cfg(test)]
    pub(super) fn platform_clipboard_slot(&self) -> Arc<Mutex<Option<Arc<dyn Clipboard>>>> {
        Arc::clone(&self.platform_clipboard)
    }
}

#[cfg(not(target_os = "ios"))]
impl Drop for AppRuntime {
    /// The third, last-resort clipboard clear: the deterministic path is the explicit
    /// `teardown_platform_realm` clear; this is only a backstop for
    /// whatever construction/panic ordering skips it. Idempotent — clearing
    /// an already-empty slot is a no-op — and must never assert platform
    /// presence: a thread-local's destructor is not guaranteed to run in any
    /// particular order relative to window/surface teardown, so this may
    /// run before, after, or never relative to those.
    fn drop(&mut self) {
        self.platform_clipboard.lock().take();
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod app_runtime_tests {
    use super::*;

    #[test]
    fn app_runtime_owns_registry_and_realm_slot() {
        let runtime = AppRuntime::new();

        assert!(
            runtime.realm.is_none(),
            "a freshly constructed AppRuntime hosts no realm yet"
        );
        assert!(
            runtime.owner_platform.is_none(),
            "a freshly constructed AppRuntime hosts no owner platform yet"
        );
        assert!(runtime.visible, "a fresh runtime assumes a visible window");
        assert!(runtime.focused, "a fresh runtime assumes a focused window");
        assert!(
            runtime.registry.is_empty(),
            "AppRuntime owns the WindowRegistry directly -- a fresh one has no mappings"
        );
        assert!(
            runtime
                .realm(RealmId::new_gen(0, NonZeroU32::new(1).unwrap()))
                .is_none(),
            "the RealmId-keyed accessor must return None when no realm is installed"
        );
    }

    /// `AppRuntime::new` must NOT resolve `SharedEngineServices` -- that is
    /// the entire point of moving resolution behind `OnceCell` +
    /// `ensure_services`. Red before the fix: `new()` used to call
    /// `SharedEngineServices::resolve()` directly, so `services` was always
    /// `Some` immediately after construction; this assertion would have
    /// failed against that shape.
    #[test]
    fn app_runtime_new_does_not_resolve_services() {
        let runtime = AppRuntime::new();

        assert!(
            runtime.services.get().is_none(),
            "AppRuntime::new must not resolve SharedEngineServices -- doing so \
             makes every first touch of the TLS slot (including an \
             OwnerHostClearGuard::drop during an unwind) run singleton \
             construction and full system-font enumeration"
        );
    }

    /// `ensure_services` must actually populate all three
    /// `SharedEngineServices` fields with live, usable handles -- reading
    /// each one here (rather than only asserting the struct compiles) is
    /// what proves the resolution seam works, not just that it type-checks.
    #[test]
    fn ensure_services_resolves_all_three_and_caches_them() {
        let mut runtime = AppRuntime::new();

        let services = runtime.ensure_services();
        let _painting_image_cache = services.painting.image_cache();
        let _accessibility_features = services.accessibility_features();
        let _phase = services.scheduler.phase();

        assert!(
            runtime.services.get().is_some(),
            "ensure_services must cache the resolved value, not re-resolve on \
             every call"
        );
    }

    /// `accessibility_features` is now a value `SharedEngineServices` owns
    /// directly (no `SemanticsBinding` singleton underneath it any more) --
    /// a set/get round-trip is the seam's own regression guard.
    #[test]
    fn set_accessibility_features_round_trips_through_shared_engine_services() {
        use flui_semantics::AccessibilityFeatures;

        let mut runtime = AppRuntime::new();
        let services = runtime.ensure_services();

        assert_eq!(
            services.accessibility_features(),
            AccessibilityFeatures::default(),
            "a freshly resolved SharedEngineServices starts with default accessibility features"
        );

        services.set_accessibility_features(AccessibilityFeatures {
            reduce_motion: true,
            ..Default::default()
        });

        assert!(services.accessibility_features().reduce_motion);
    }

    /// A `OwnerHostClearGuard`-shaped operation that touches ONLY
    /// `owner_platform` -- never `ensure_services` -- must leave `services`
    /// unresolved. Mirrors `OwnerHostClearGuard::drop`'s actual field touch
    /// (`runner.rs`) without depending on `runner.rs`'s platform machinery.
    #[test]
    fn guard_only_arm_and_drop_cycle_never_resolves_services() {
        let mut runtime = AppRuntime::new();

        // The clear-guard's drop body: `self.owner_platform.take();` and
        // nothing else.
        runtime.owner_platform.take();

        assert!(
            runtime.services.get().is_none(),
            "a guard-only arm/drop cycle (owner_platform touch only) must \
             never resolve SharedEngineServices -- that would reintroduce \
             the double-panic-during-unwind hazard this shape closes"
        );
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod wake_and_clipboard_tests {
    use super::*;

    /// `wake_frame` must set `needs_redraw` even when no window is stored
    /// (the window lock is a leaf that is independently optional).
    #[test]
    fn wake_frame_sets_needs_redraw_without_window() {
        let runtime = AppRuntime::new();
        runtime.mark_rendered();
        assert!(!runtime.needs_redraw(), "precondition: no redraw pending");

        runtime.wake_frame();

        assert!(
            runtime.needs_redraw(),
            "wake_frame must set needs_redraw even without an active window"
        );
    }

    /// `wake_frame` must call `PlatformWindow::request_redraw` when a window
    /// is installed.
    #[test]
    fn wake_frame_calls_platform_request_redraw() {
        use std::sync::atomic::AtomicU32;

        use flui_platform::traits::PlatformWindow;
        use flui_types::geometry::{DevicePixels, Pixels, Size, device_px, px};

        struct CountingWindow {
            redraw_count: Arc<AtomicU32>,
        }

        impl PlatformWindow for CountingWindow {
            fn id(&self) -> flui_platform::traits::WindowId {
                flui_platform::traits::WindowId(1)
            }
            fn physical_size(&self) -> Size<DevicePixels> {
                Size::new(device_px(800), device_px(600))
            }
            fn logical_size(&self) -> Size<Pixels> {
                Size::new(px(800.0), px(600.0))
            }
            fn scale_factor(&self) -> f64 {
                1.0
            }
            fn request_redraw(&self) {
                self.redraw_count.fetch_add(1, Ordering::Relaxed);
            }
            fn is_focused(&self) -> bool {
                false
            }
            fn is_visible(&self) -> bool {
                true
            }
            fn set_cursor(
                &self,
                _cursor: flui_platform::CursorIcon,
            ) -> Result<(), flui_platform::CursorError> {
                Ok(())
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let redraw_count = Arc::new(AtomicU32::new(0));
        let window = CountingWindow {
            redraw_count: Arc::clone(&redraw_count),
        };

        let runtime = AppRuntime::new();
        runtime.mark_rendered();
        runtime.set_redraw_window(Arc::new(window));

        runtime.wake_frame();

        assert!(runtime.needs_redraw(), "wake_frame must set needs_redraw");
        assert_eq!(
            redraw_count.load(Ordering::Relaxed),
            1,
            "wake_frame must call PlatformWindow::request_redraw exactly once"
        );
    }

    /// `AppRuntime::clipboard()` reaching the platform clipboard installed
    /// via `set_platform_clipboard` — migrated from the retired
    /// `AppBinding`'s test module.
    #[test]
    fn app_runtime_clipboard_reaches_the_installed_platform_clipboard() {
        let runtime = AppRuntime::new();
        assert!(
            runtime.clipboard().is_none(),
            "no platform installed yet must read back as None"
        );

        let clipboard = flui_platform::headless_platform().clipboard();
        runtime.set_platform_clipboard(Arc::clone(&clipboard));

        let reached = runtime
            .clipboard()
            .expect("set_platform_clipboard must make the clipboard reachable");
        reached.write_text("clipboard-reachability".to_string());

        assert_eq!(
            runtime
                .clipboard()
                .expect("still installed")
                .read_text()
                .as_deref(),
            Some("clipboard-reachability"),
            "AppRuntime::clipboard() must reach through to the SAME platform \
             clipboard instance set_platform_clipboard installed"
        );
    }

    #[test]
    fn clipboard_with_no_platform_installed_is_none_not_a_panic() {
        let runtime = AppRuntime::new();
        assert!(runtime.clipboard().is_none());
    }

    /// Teardown symmetry: after `clear_platform_clipboard`, the slot reads
    /// back `None` again.
    #[test]
    fn clear_platform_clipboard_removes_the_installed_clipboard() {
        let runtime = AppRuntime::new();
        runtime.set_platform_clipboard(flui_platform::headless_platform().clipboard());
        assert!(runtime.clipboard().is_some());

        runtime.clear_platform_clipboard();

        assert!(
            runtime.clipboard().is_none(),
            "clear_platform_clipboard must remove the installed clipboard"
        );
    }

    /// The last-resort `Drop` clear: dropping an `AppRuntime` with a
    /// clipboard still installed must not panic, and must clear the slot.
    #[test]
    fn drop_clears_the_installed_clipboard_as_a_last_resort() {
        let runtime = AppRuntime::new();
        let slot = runtime.platform_clipboard_slot();
        runtime.set_platform_clipboard(flui_platform::headless_platform().clipboard());
        assert!(slot.lock().is_some());

        drop(runtime);

        assert!(
            slot.lock().is_none(),
            "Drop for AppRuntime must clear the clipboard slot as a last resort"
        );
    }

    /// A reentrant read through the exact same slot must not deadlock — the
    /// clone-then-drop-guard discipline `clipboard()` follows.
    #[test]
    fn clipboard_reentrant_read_does_not_deadlock() {
        use std::sync::mpsc;
        use std::time::Duration;

        struct ReentrantClipboard {
            slot: Arc<Mutex<Option<Arc<dyn Clipboard>>>>,
        }

        impl Clipboard for ReentrantClipboard {
            fn read_text(&self) -> Option<String> {
                let reentered = self.slot.lock().clone();
                assert!(
                    reentered.is_some(),
                    "reentrant read through the same slot must still see the installed clipboard"
                );
                Some("reentrant".to_string())
            }

            fn write_text(&self, _text: String) {}
        }

        let (result_tx, result_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = AppRuntime::new();
            let slot = runtime.platform_clipboard_slot();
            runtime.set_platform_clipboard(Arc::new(ReentrantClipboard { slot }));

            let reached = runtime.clipboard().expect("clipboard installed above");
            let text = reached.read_text();
            let _ = result_tx.send(text);
        });

        let text = result_rx.recv_timeout(Duration::from_secs(5)).expect(
            "AppRuntime::clipboard() deadlocked: a reentrant read_text call must not block on \
             the platform_clipboard lock it itself just released",
        );
        assert_eq!(text.as_deref(), Some("reentrant"));
    }

    /// Serializes tests that drive the owner thread's `Scheduler::instance()`
    /// singleton, mirroring `ui_realm.rs`'s `SINGLETON_FRAME_LOCK` (both
    /// guard the same kind of state for the same reason).
    static WAKE_HOOK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `ensure_services` must wire the animation-wake hook as the
    /// install-time-captured `Send + Sync` handle
    /// [`AppRuntime::frame_wake_callback`] returns, not a callback that
    /// re-resolves this thread-local `AppRuntime` when the hook fires.
    ///
    /// Proof shape: spawn a task on the real `Scheduler::instance()`,
    /// capture its `Waker`, then fire that `Waker` from an OS thread that
    /// never touches `runtime`, `APP_RUNTIME`, or `Scheduler::instance()` —
    /// and observe `needs_redraw` flip on the ORIGINAL runtime anyway. A
    /// hook built by re-resolving `APP_RUNTIME` at fire time instead of
    /// capturing this `Send` handle would see an empty thread-local on the
    /// foreign thread and never flip this flag — the revert recipe for this
    /// test. (A real instance of exactly this mistake shipped once, in the
    /// frames-reenable-redirty logic — see `emit_lifecycle_transition`'s
    /// doc in `runner.rs` for that story and its fix.)
    #[test]
    fn ensure_services_installs_a_send_wake_hook_that_survives_a_cross_thread_fire() {
        use std::sync::mpsc;
        use std::task::Waker;
        use std::time::Duration;

        let _serialized = WAKE_HOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut runtime = AppRuntime::new();
        let scheduler = runtime.ensure_services().scheduler.get();
        assert!(!runtime.needs_redraw(), "precondition: no redraw pending");

        let stored_waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let stored_for_task = Arc::clone(&stored_waker);
        let _token = scheduler.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            *stored_for_task.lock() = Some(cx.waker().clone());
            std::task::Poll::<()>::Pending
        })));

        // `spawn_local` itself already requested (and thus already woke) a
        // frame; consume that pending flag as a real frame would, so the
        // cross-thread wake below is the false->true edge under test.
        scheduler.handle_begin_frame(flui_scheduler::Instant::now());
        scheduler.drive_async_tasks();
        runtime.mark_rendered();
        assert!(
            !runtime.needs_redraw(),
            "consuming the spawn-time wake must not leave a stale flag"
        );

        let waker = stored_waker
            .lock()
            .clone()
            .expect("waker stored by the poll above");

        let (fired_tx, fired_rx) = mpsc::channel();
        std::thread::spawn(move || {
            // Deliberately touches nothing but the waker itself: no
            // `runtime`, no `APP_RUNTIME`, no `Scheduler::instance()` on
            // this thread.
            waker.wake();
            let _ = fired_tx.send(());
        });
        fired_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the foreign thread must be able to fire the waker without blocking");

        assert!(
            runtime.needs_redraw(),
            "the animation-wake hook ensure_services installed must be a Send handle captured \
             at install time, not one resolved from a thread-local at fire time -- a foreign \
             OS thread has no such thread-local to resolve"
        );
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn next_identity_mints_distinct_generations() {
        let (realm_a, _) = next_identity();
        let (realm_b, _) = next_identity();
        assert_ne!(
            realm_a, realm_b,
            "every mint must produce a fresh generation, never repeating"
        );
    }
}
