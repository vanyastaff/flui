//! `AppRuntime` — the loop-scoped composition root.
//!
//! # Thesis
//!
//! This module creates the composition root and the constructor-injection
//! seams while every service it names is still singleton-*backed*. The
//! honest claim: `UiRealm` performs zero `::instance()` calls; the services
//! it consumes are resolved once, here, in [`SharedEngineServices::resolve`].
//! Other ambient reaches (`renderer_binding.rs`, `binding.rs`,
//! `hot_reload.rs`, `config.rs`, `text.rs`) are untouched — they remain
//! until the change that retires each singleton they reach for. This is not
//! a forwarding shim: no old API is preserved-but-deprecated here, and no
//! ambient access point this change does not touch is claimed as closed.
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
    fn resolve() -> Self {
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
use std::thread::ThreadId;

#[cfg(not(target_os = "ios"))]
use flui_foundation::PresentationAddress;
#[cfg(not(target_os = "ios"))]
use flui_painting::PaintingBinding;
#[cfg(not(target_os = "ios"))]
use flui_platform::OwnerPlatform;
#[cfg(not(target_os = "ios"))]
use flui_semantics::SemanticsBinding;

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
/// Every field here is transitional: the type each names is still the
/// process-global singleton underneath (`PaintingBinding::instance()` and
/// friends) — only the *resolution point* moves to this one constructor.
/// Each field's `'static` reference is expected to flip to an owned value
/// once the change that retires that singleton lands (painting, semantics,
/// and the scheduler each have their own separate follow-up); until then
/// these fields are `pub(super)`, not part of any public surface (ADR-0027
/// §9: transitional runtime types stay `pub(crate)` at most).
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
    pub(super) painting: &'static PaintingBinding,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
        )
    )]
    pub(super) semantics: &'static SemanticsBinding,
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
    fn resolve() -> Self {
        let painting = PaintingBinding::instance();

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

        Self {
            painting,
            semantics: SemanticsBinding::instance(),
            scheduler: SchedulerRef::resolve(),
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
    /// existing `UiRealm` constructor (`new`, `with_capacity`, `for_test`,
    /// `for_test_with_text_input`) keep its exact current signature here:
    /// each calls this instead of reaching for `Scheduler::instance()`
    /// directly. Rewriting `for_test`/`for_test_with_text_input` onto
    /// explicit `RealmServices` + `PresentationState` injection (dropping
    /// `&AppBinding` entirely) is a separate, later change, not this one.
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
}

#[cfg(not(target_os = "ios"))]
impl AppRuntime {
    /// Construct the composition root: cheap, side-effect-free, `const`.
    /// Called as the `APP_RUNTIME` TLS slot's own `const` initializer
    /// (`runner.rs`), so simply *touching* the thread-local -- for any
    /// reason, on any thread -- can never itself run singleton construction
    /// or full system-font enumeration. Real service resolution happens
    /// only via the explicit [`Self::ensure_services`] call from
    /// `install_platform_realm` -- when a realm is actually installed --
    /// never from an incidental first touch such as
    /// `OwnerHostClearGuard::drop` unwinding through a virgin thread, and
    /// never from `install_owner_platform` either (every backend calls
    /// that, including `run_direct`, which never installs a realm and
    /// never needs these services).
    pub(super) const fn new() -> Self {
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
        self.services.get_or_init(SharedEngineServices::resolve)
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
        let _semantics_features = services.semantics.accessibility_features();
        let _phase = services.scheduler.phase();

        assert!(
            runtime.services.get().is_some(),
            "ensure_services must cache the resolved value, not re-resolve on \
             every call"
        );
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
