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

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_foundation::{PresentationId, RealmId};
use flui_scheduler::{AsyncDriver, LocalPostFrameLane, Scheduler};

// `RealmServices` and `next_identity` below are used unconditionally by
// `ui_realm.rs` (every `UiRealm` constructor resolves its own
// `RealmServices` now, on every platform `UiRealm` itself compiles for,
// including iOS's stub). `AppRuntime` and `SharedEngineServices` further
// down are the loop-scoped composition root that only the non-iOS runners
// (`runner.rs`'s desktop/android/web dispatch) instantiate, so they -- and
// the imports only they need -- stay `#[cfg(not(target_os = "ios"))]`,
// matching the cfg the absorbed `RealmHost`/`OWNER_PLATFORM_HOST` carried.

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
use super::window_registry::{RegistryError, WindowRegistry};

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
/// now owns that value directly. There is no `scheduler` field here any
/// more: each realm now owns its own `Scheduler` strong root (see
/// [`RealmServices::construct`]), so there is no process-level scheduler
/// left for this struct to resolve.
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

    /// Resolves the process-level services that survive a scheduler that is
    /// no longer process-global. Called once per owner thread
    /// (`ensure_services`'s `OnceCell::get_or_init`), the same steal-proof,
    /// idempotent guarantee the retired `AppBinding::instance()`'s
    /// thread-local initializer gave.
    fn resolve() -> Self {
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

        Self {
            painting,
            accessibility_features: RwLock::new(AccessibilityFeatures::default()),
        }
    }
}

/// What [`UiRealm::construct`](super::ui_realm) needs to wire itself up: a
/// fresh, realm-owned [`Scheduler`] — the strong root — plus the
/// `local_post_frame_lane()` and `async_driver()` handles derived from that
/// SAME scheduler (formerly `Scheduler::instance()` calls inside
/// `ui_realm.rs` itself). Resolved once, here — never inside `ui_realm.rs`
/// itself, so `UiRealm`'s own source performs zero `::instance()` calls.
pub(crate) struct RealmServices {
    pub(crate) local_post_frame: LocalPostFrameLane,
    pub(crate) async_driver: AsyncDriver,
    pub(crate) scheduler: Scheduler,
}

impl RealmServices {
    /// Builds a brand-new `Scheduler` for a realm about to be constructed.
    /// Every `UiRealm` constructor (`new`, `with_capacity`, `for_test`,
    /// `for_test_with_text_input`) calls this instead of reaching for a
    /// process-global scheduler — each realm gets its OWN strong root, torn
    /// down when the realm drops, none of them taking a process-host
    /// parameter any more (the retired `AppBinding` is gone).
    pub(crate) fn construct() -> Self {
        let scheduler = Scheduler::new();
        Self {
            local_post_frame: scheduler.local_post_frame_lane(),
            async_driver: scheduler.async_driver().clone(),
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

// ============================================================================
// Multi-realm hosting (issue #555)
// ============================================================================

/// One realm's dispatch-adjacent state, keyed by [`RealmId`] in
/// [`AppRuntime`]'s [`RealmRegistry`]. Bundles exactly what the single-slot
/// design this replaces used to keep as flat `AppRuntime` fields (`realm`,
/// `queue`, `draining`, `address`, `surface_applier`): each hosted realm now
/// owns its own copy of all five, so a second realm's dispatch, resize
/// routing, and reentrancy guard are independent of the first's — the
/// concrete mechanism behind the end-state invariant that two windows share
/// no mutable UI tree through `AppRuntime` (sharing happens only through
/// explicit `SharedEngineServices`/app-model injection, never through this
/// registry).
#[cfg(not(target_os = "ios"))]
pub(super) struct RealmSlot {
    /// `None` while this realm is checked OUT of the registry for
    /// [`dispatch_platform_realm`](super::runner) — the other four fields
    /// stay in place throughout that checkout (never removed alongside
    /// `realm`), so a nested same-realm dispatch still finds its target and
    /// enqueues into `queue`, instead of reading back `StaleRealm`.
    pub(super) realm: Option<UiRealm>,
    /// This realm's own owner-thread work queue — never shared with a
    /// sibling realm's queue, so draining one realm's events can never pop a
    /// task meant for another. Each entry is stamped with the
    /// [`PresentationId`] of the `RealmDispatcher` (`runner.rs`, private to
    /// that module) that enqueued it (issue #555's addressed-routing slice): a
    /// realm's queue is shared across every presentation it hosts, so which
    /// window produced a given `RealmTask::Event` cannot be recovered from
    /// the queue's OWN dispatch call alone once more than one presentation's
    /// window can enqueue into it — the stamp travels with the task itself
    /// instead. `RealmTask::Frame`/`ClosePresentation` ignore their stamp
    /// (frame pump is realm-wide; `ClosePresentation` already carries its
    /// own target id as its payload) — only `RealmTask::Event` reads it, in
    /// `PlatformToUi::run` (`runner.rs`).
    pub(super) queue: VecDeque<(PresentationId, RealmTask)>,
    /// Set while a queued task for THIS realm is running, so a reentrant
    /// same-realm dispatch enqueues instead of recursing into `realm.take()`.
    pub(super) draining: bool,
    /// This realm's routable address — the per-slot replacement for the
    /// single `AppRuntime.address: Option<PresentationAddress>` this type
    /// used to be.
    pub(super) address: PresentationAddress,
    /// This realm's own registration-lifetime renderer-surface applier for a
    /// `Resized` event — never shared with a sibling realm's applier, so a
    /// resize addressed to one window can never resize another's surface.
    pub(super) surface_applier: Option<SurfaceApplier>,
}

/// The [`RealmId`]-keyed, insertion-ordered realm registry `AppRuntime` hosts
/// — the multi-realm replacement for the single `Option<UiRealm>` slot (plus
/// its four sibling flat fields) this type used to be.
///
/// Insertion order matters: hot-restart's [`super::runner`]
/// `for_each_installed_realm` and the exit-policy drain both visit realms in
/// the order they were installed, i.e. mount order.
///
/// Storage is a linear-scan `Vec`, not a hash map: the number of live realms
/// is the number of open top-level windows, small enough that O(n) lookup is
/// not a real cost, and a `Vec` gives insertion order for free with no extra
/// bookkeeping — the same reasoning `WindowRegistry` already applies to its
/// own `Vec<(WindowId, PresentationAddress)>` storage.
#[cfg(not(target_os = "ios"))]
#[derive(Default)]
pub(super) struct RealmRegistry {
    slots: Vec<(RealmId, RealmSlot)>,
}

#[cfg(not(target_os = "ios"))]
impl RealmRegistry {
    pub(super) const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "shared-reference lookup; every production call site so far checks a slot \
                      out via get_mut (the checkout-based dispatch/visit pattern) -- exercised \
                      by this crate's own tests, which only need to read a slot back, never \
                      mutate it"
        )
    )]
    pub(super) fn get(&self, id: &RealmId) -> Option<&RealmSlot> {
        self.slots
            .iter()
            .find(|(slot_id, _)| slot_id == id)
            .map(|(_, slot)| slot)
    }

    pub(super) fn get_mut(&mut self, id: &RealmId) -> Option<&mut RealmSlot> {
        self.slots
            .iter_mut()
            .find(|(slot_id, _)| slot_id == id)
            .map(|(_, slot)| slot)
    }

    pub(super) fn contains_key(&self, id: &RealmId) -> bool {
        self.slots.iter().any(|(slot_id, _)| slot_id == id)
    }

    /// Inserts `slot` at `id`, appending at the end of insertion order.
    ///
    /// `id` must not already be present: `next_identity` never repeats a
    /// `RealmId`, and every removal path (`remove`/`clear`) drops the old
    /// entry before a replacement is ever inserted, so a collision here
    /// means a stale id was reused — a bug this debug_assert catches instead
    /// of silently shadowing a live realm.
    pub(super) fn insert(&mut self, id: RealmId, slot: RealmSlot) {
        debug_assert!(
            !self.contains_key(&id),
            "BUG: RealmRegistry::insert called for an id already present -- \
             next_identity never repeats a RealmId, so this means a stale id was reused"
        );
        self.slots.push((id, slot));
    }

    /// Removes and returns the slot at `id`, if present.
    pub(super) fn remove(&mut self, id: &RealmId) -> Option<RealmSlot> {
        let index = self.slots.iter().position(|(slot_id, _)| slot_id == id)?;
        Some(self.slots.remove(index).1)
    }

    /// Removes and returns every slot, in insertion order — the
    /// multi-realm generalization of `mem::take`-ing the old single
    /// `Option` slot (`install_platform_realm`'s reinstall-without-teardown
    /// path, and `teardown_platform_realm`'s full loop-exit teardown, both
    /// use this).
    pub(super) fn clear(&mut self) -> Vec<(RealmId, RealmSlot)> {
        std::mem::take(&mut self.slots)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &(RealmId, RealmSlot)> {
        self.slots.iter()
    }

    /// Every installed `RealmId`, in insertion (mount) order — the read
    /// `for_each_installed_realm` (`super::runner`) snapshots before it
    /// starts checking realms out one at a time.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "for_each_installed_realm's only production driver is a follow-up; \
                      exercised by this module's own tests via that function"
        )
    )]
    pub(super) fn keys(&self) -> Vec<RealmId> {
        self.slots.iter().map(|(id, _)| *id).collect()
    }
}

/// A realm-registry install/uninstall requested while a mutation must defer
/// (a dispatch or an all-realms iteration is in flight on this thread — see
/// [`AppRuntime::request_realm_install`]/[`AppRuntime::request_realm_uninstall`]).
/// Applied in request order once the deferring condition clears. Private:
/// external callers (`super::runner`) go through the typed
/// `request_realm_install`/`request_realm_uninstall` methods, never
/// construct this enum directly — keeping the window-registration step
/// (see [`AppRuntime::apply_install`]) bundled with the registry insert
/// atomically, instead of requiring every call site to remember both.
#[cfg(not(target_os = "ios"))]
enum RealmMapMutation {
    /// Add a newly-constructed realm to the registry (never displaces a
    /// sibling — see `install_realm_alongside` in `super::runner`), plus the
    /// window whose id mints its `WindowRegistry` mapping. Boxed: `RealmSlot`
    /// owns a whole `UiRealm`, over a kilobyte, next to `Uninstall`'s bare
    /// `RealmId` -- boxing keeps this enum (and every `Vec<RealmMapMutation>`
    /// queueing it) from paying that size for every entry regardless of
    /// variant.
    Install(RealmId, Box<RealmSlot>, Arc<dyn PlatformWindow>),
    /// Remove one realm from the registry (a window closing while siblings
    /// stay open — see `uninstall_platform_realm` in `super::runner`).
    Uninstall(RealmId),
}

/// Governs when the platform loop should exit once every hosted realm's
/// window has closed — the embedder-facing policy knob for the "new
/// independent desktop window ⇒ new realm" production policy (ADR-0027 step
/// 5's multi-window follow-up, issue #555).
///
/// Consulted through [`AppRuntime::should_exit`], which drains any deferred
/// realm-map mutation FIRST (the drain-before-decide rule): a
/// queued "open another window" install — e.g. a splash screen's dispose
/// callback requesting the main window — is applied before the
/// empty-registry check, so it vetoes exit instead of racing it. Without
/// that ordering, a splash-close and a main-window-open landing in the same
/// idle tick could observe "no realms installed" and exit before the queued
/// install ever lands.
///
/// `#[non_exhaustive]`: a future variant (e.g. "never exit automatically;
/// the embedder calls `quit()` itself") must not be a breaking change for a
/// caller that already matches exhaustively — the same precedent
/// `AttachError` set (`binding.rs`).
///
/// Not yet wired into a live platform loop: `run_desktop`/`run_android`/
/// `run_web` still let each backend's own window-close handling
/// (`flui-platform`'s per-backend `CloseRequested`/`on_should_close`
/// handling) decide exit unconditionally. This type and
/// `AppRuntime::should_exit` are the decision *mechanism* — the policy shape
/// is the deliverable now, wiring a real winit multi-window loop through it
/// is a follow-up (tracked as this issue's native-lifecycle slice).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the exit-policy mechanism has no production embedder call site yet -- \
                  wiring a real winit multi-window loop through it is this issue's \
                  native-lifecycle follow-up; exercised by this crate's own tests"
    )
)]
pub enum ExitPolicy {
    /// Exit once every hosted realm has closed and none is queued to open.
    /// The default policy.
    #[default]
    OnLastWindowClosed,
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
/// The realm-facing API is `RealmId`-keyed: `realms` is [`RealmRegistry`],
/// an insertion-ordered map of any number of hosted realms (issue #555) —
/// `next_identity` above already mints from a shape that never needed to
/// change for this to land.
#[cfg(not(target_os = "ios"))]
pub(crate) struct AppRuntime {
    /// Every hosted realm, keyed by `RealmId`, in mount (insertion) order.
    /// Replaces the single `Option<UiRealm>` slot (plus its four sibling
    /// flat fields `queue`/`draining`/`address`/`surface_applier`) this
    /// struct used to carry — see [`RealmSlot`]'s doc for why those four
    /// moved inside the per-realm entry instead of staying flat.
    pub(super) realms: RealmRegistry,
    /// The thread that installed the first realm hosted here; every dispatch
    /// checks against this before touching the registry. Loop-scoped, not
    /// per-realm: every realm this `AppRuntime` ever hosts lives on the same
    /// owner thread (`APP_RUNTIME` is thread-local), so one shared value is
    /// exact, not an approximation of a per-realm concept.
    pub(super) owner_thread: Option<ThreadId>,
    /// The sole native-window-to-presentation mapping authority (ADR-0037
    /// §2), already `RealmId`-keyed and multi-window-shaped — see its own
    /// module doc.
    pub(super) registry: WindowRegistry,
    /// Single-window `(visible, focused)` tracking for the
    /// `AppLifecycleState` derivation (ADR-0035). Both default `true`.
    ///
    /// **Known limitation, stated rather than silently overclaimed:** this
    /// pair is still loop-scoped, not per-realm — with more than one
    /// hosted realm, a focus/visibility change on any one window's OS
    /// signal drives the SAME derivation for the whole loop. Per-realm
    /// lifecycle derivation is `FocusCoordinator` territory (a later, exact-routing
    /// slice of issue #555), out of this change's scope.
    pub(super) visible: bool,
    pub(super) focused: bool,
    /// The loop-scoped owner-thread platform capability (ADR-0039 §6).
    /// Deliberately *not* cleared by realm teardown — the loop may host
    /// another realm before it exits (hot-restart does exactly this).
    pub(super) owner_platform: Option<OwnerPlatform>,
    /// A clone of the currently-dispatched realm's scheduler, held ONLY
    /// while `dispatch_platform_realm` (in `runner.rs`) has taken that
    /// realm's slot out of `realms` above for the duration of a queued task.
    /// Without this, [`Self::installed_realm_phase`] (`with_owner_platform`'s
    /// fence (c)) reads `None` for the realm's entire dispatched extent —
    /// not just when no realm is installed at all — because
    /// `dispatch_platform_realm` checks the realm's `UiRealm` OUT of its slot
    /// before running any task, including the frame pump that drives the
    /// scheduler through `PersistentCallbacks`. That makes the fence blind
    /// exactly when a frame phase is actually running, which is the one case
    /// trigger #22 exists to catch. `Scheduler` is a single-`Arc` handle (see
    /// `flui-scheduler`'s `Scheduler`/`SchedulerInner` split), so cloning it
    /// here to survive the checkout is cheap — an `Arc::clone`, not a new
    /// scheduler. Set at checkout, cleared at restore
    /// (`dispatch_platform_realm`), in both cases inside the same
    /// `catch_unwind`-guarded block that restores the realm's slot itself, so
    /// an unwinding dispatched task leaves this `None` exactly as reliably as
    /// it leaves the slot restored.
    ///
    /// Stays single-slot on purpose: dispatch is
    /// single-threaded and sequential, so at most one realm is EVER checked
    /// out on this thread at a given instant — [`RealmMapMutation`]'s
    /// defer-to-idle discipline exists precisely so no second, nested
    /// dispatch for a DIFFERENT realm is ever attempted while this is
    /// `Some`; `dispatch_platform_realm` debug-asserts that invariant at its
    /// one checkout site.
    pub(super) dispatched_scheduler: Option<Scheduler>,
    /// The identity companion to `dispatched_scheduler` above: which realm
    /// is currently checked out, so a nested dispatch attempt targeting a
    /// DIFFERENT realm can be told apart from a legitimate same-realm
    /// reentrant call (already handled by each slot's own `draining` flag).
    /// Also set (alongside `dispatched_scheduler`) for the realm
    /// `for_each_installed_realm` currently has checked out of the registry
    /// mid-visit — from fence (c)'s perspective a visited realm IS
    /// dispatched: its own scheduler phase must still be observable, not
    /// blind, for the whole time it sits outside `realms`.
    pub(super) dispatched_realm_id: Option<RealmId>,
    /// Set for the duration of `for_each_installed_realm`'s (`runner.rs`)
    /// hot-restart-shaped visit over every hosted realm. A realm-map
    /// mutation requested while this is `true` defers exactly like one
    /// requested while `dispatched_realm_id` is `Some` — see
    /// [`Self::request_realm_install`]/[`Self::request_realm_uninstall`] —
    /// so a mutation triggered by a callback running mid-visit never
    /// changes the set of realms that same visit is still walking.
    pub(super) iterating_all_realms: bool,
    /// Realm-map mutations requested while
    /// [`Self::request_realm_install`]/[`Self::request_realm_uninstall`]
    /// decided they must defer. Applied, in request order, by
    /// [`Self::drain_pending_realm_mutations`].
    pending_realm_mutations: Vec<RealmMapMutation>,
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
            realms: RealmRegistry::new(),
            owner_thread: None,
            registry: WindowRegistry::new(),
            visible: true,
            focused: true,
            owner_platform: None,
            dispatched_scheduler: None,
            dispatched_realm_id: None,
            iterating_all_realms: false,
            pending_realm_mutations: Vec::new(),
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

    /// True if `phase` is one of the phases `with_owner_platform`'s fence (c)
    /// forbids: a frame transaction genuinely in flight, as opposed to
    /// `Idle`/`PostFrameCallbacks`, both of which are legitimate times to
    /// acquire owner-platform capability (ADR-0021-style post-frame work).
    fn is_frame_transaction_phase(phase: flui_scheduler::SchedulerPhase) -> bool {
        matches!(
            phase,
            flui_scheduler::SchedulerPhase::TransientCallbacks
                | flui_scheduler::SchedulerPhase::MidFrameMicrotasks
                | flui_scheduler::SchedulerPhase::PersistentCallbacks
        )
    }

    /// The installed realm's scheduler phase, or `None` if no realm is
    /// installed on this thread AND no realm is currently checked out for
    /// dispatch either — the replacement for `with_owner_platform`'s fence
    /// (c), formerly a bare `Scheduler::instance().phase()` read.
    ///
    /// **Design-for-N (issue #555): iterates every resident slot in
    /// `realms`**, rather than assuming a single slot is "the" realm that
    /// matters — but checks [`Self::dispatched_scheduler`] FIRST: dispatch is
    /// single-threaded and sequential, so whenever a realm is checked out for
    /// `dispatch_platform_realm` (the entire extent of a queued task,
    /// including the frame pump that drives `PersistentCallbacks`), every
    /// OTHER resident realm is necessarily `Idle`/`PostFrameCallbacks` (no
    /// two realms ever run their frame pump concurrently on one thread) —
    /// reading `dispatched_scheduler` first is therefore both sufficient and
    /// exact for that case, not merely a fallback. Only once nothing is
    /// checked out does this fall through to scanning `realms` itself, so an
    /// addressed probe (this dispatch's own realm) and the aggregate fence
    /// both stay correct without duplicating the forbidden-phase list.
    /// Single-pass, no intermediate allocation: returns the first FORBIDDEN
    /// phase found, short-circuiting the scan; if none is forbidden, returns
    /// the first realm's OBSERVED phase honestly (never claimed to be
    /// specifically `Idle` — a fully quiescent realm can legitimately sit in
    /// `PostFrameCallbacks` just as validly, and this reads back whichever
    /// one it actually is) rather than `None`, matching this function's
    /// pre-registry single-slot behavior for the common one-realm case (a
    /// realm being installed reads back its own real phase, not a
    /// vacuous "nothing installed" `None`).
    pub(super) fn installed_realm_phase(&self) -> Option<flui_scheduler::SchedulerPhase> {
        if let Some(scheduler) = self.dispatched_scheduler.as_ref() {
            return Some(scheduler.phase());
        }
        let mut first_observed = None;
        for (_, slot) in self.realms.iter() {
            let Some(realm) = slot.realm.as_ref() else {
                continue;
            };
            let phase = realm.scheduler().phase();
            if Self::is_frame_transaction_phase(phase) {
                return Some(phase);
            }
            first_observed.get_or_insert(phase);
        }
        first_observed
    }

    /// The un-deferred application of an `Install` mutation: registers
    /// `window` in the `WindowRegistry` FIRST, strictly (never replacing an
    /// existing mapping — an id collision is refused, not silently
    /// re-routed onto a sibling realm's window), and only inserts `slot`
    /// into `realms` once that registration succeeds. Hands `window` to
    /// [`super::window_registry::WindowRegistry::try_register_window`]
    /// rather than deriving its id here: `WindowId` is the registry's own
    /// single-authority concern (ADR-0037 §2) — `AppRuntime` never names or
    /// touches it directly.
    ///
    /// On `Err`, hands `slot` BACK rather than dropping it here: this method
    /// is always called while some caller's `APP_RUNTIME` `RefCell` borrow is
    /// still live, and `slot` owns a whole `UiRealm` whose destructors may
    /// re-enter platform/framework code — the same reason
    /// `install_platform_realm`/`teardown_platform_realm` never drop a
    /// realm-owning value while their own TLS borrow is held. Every caller of
    /// this method routes the returned slot through its own `removed`-style
    /// return value instead, so the actual drop happens only once that
    /// borrow has released.
    fn apply_install(
        &mut self,
        id: RealmId,
        slot: RealmSlot,
        window: &Arc<dyn PlatformWindow>,
    ) -> Result<(), (RegistryError, Box<RealmSlot>)> {
        if let Err(error) = self.registry.try_register_window(window, slot.address) {
            return Err((error, Box::new(slot)));
        }
        self.realms.insert(id, slot);
        Ok(())
    }

    /// The un-deferred application of an `Uninstall` mutation.
    ///
    /// Registry removal runs first, matching `window_registry.rs`'s
    /// module-doc invariant and `teardown_platform_realm`'s own ordering:
    /// stop new routing before the returned `RealmSlot`'s queued
    /// old-generation events are ever dropped (whenever the caller
    /// eventually drops it — see this module's TLS-borrow-reentrancy
    /// discipline for why that drop happens outside this function, not
    /// inside it).
    fn apply_uninstall(&mut self, id: RealmId) -> Option<RealmSlot> {
        self.registry.remove_realm(id);
        self.realms.remove(&id)
    }

    /// Requests installing a newly-constructed realm, registering `window`'s
    /// id and inserting `slot` into the registry TOGETHER, atomically from
    /// every caller's perspective: either both happen now, or both are
    /// queued as one [`RealmMapMutation::Install`] entry and both happen
    /// together once the queue drains. This closes the gap a two-step
    /// "register the window now, defer the realm insert" sequence would
    /// otherwise leave open — a platform event addressed to the new
    /// window's id arriving in that gap would find a `WindowRegistry` entry
    /// but no matching `realms` entry, and `dispatch_platform_realm` would
    /// misreport it as `StaleRealm` ("a newer realm replaced the one it was
    /// dispatched for") instead of "this realm's install has not landed yet".
    ///
    /// Applied immediately unless a dispatch (`dispatched_realm_id` is
    /// `Some`) or an all-realms iteration (`iterating_all_realms`) is
    /// currently in flight on this thread, in which case it queues instead —
    /// the mechanism behind two contracts: an install requested from inside a
    /// dispatched frame callback never nests into a second, concurrent
    /// dispatch (it lands once the outer dispatch's restore completes,
    /// `dispatch_platform_realm`'s tail), and a mutation requested
    /// mid-hot-restart-visit never changes the set of realms
    /// `for_each_installed_realm` is still walking (its own tail).
    ///
    /// `Err` only when applied immediately AND the window's id already maps
    /// to a live entry — deferred installs cannot fail synchronously (the
    /// caller has already returned by the time they apply); see
    /// [`Self::drain_pending_realm_mutations`] for how that case is handled
    /// instead (traced and dropped, never silently re-routed). On that
    /// immediate `Err`, hands `slot` back inside the error (see
    /// [`Self::apply_install`]'s own doc for why) — the caller (always
    /// itself inside a live `APP_RUNTIME` borrow) must drop it only after
    /// that borrow releases.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "install_realm_alongside is design-for-N mechanism with no production \
                      embedder call site yet -- exercised by this crate's own tests"
        )
    )]
    pub(super) fn request_realm_install(
        &mut self,
        id: RealmId,
        slot: RealmSlot,
        window: Arc<dyn PlatformWindow>,
    ) -> Result<(), (RegistryError, Box<RealmSlot>)> {
        if self.dispatched_realm_id.is_some() || self.iterating_all_realms {
            self.pending_realm_mutations.push(RealmMapMutation::Install(
                id,
                Box::new(slot),
                window,
            ));
            return Ok(());
        }
        self.apply_install(id, slot, &window)
    }

    /// Requests uninstalling one realm — a single window closing while
    /// siblings stay open. Same defer-to-idle discipline as
    /// [`Self::request_realm_install`]; see that method's doc for why.
    ///
    /// Returns any [`RealmSlot`] an IMMEDIATE uninstall removed, so the
    /// caller can drop it (and the `UiRealm` it owns) only after releasing
    /// whatever `APP_RUNTIME` borrow is live — the same discipline
    /// `install_platform_realm`/`teardown_platform_realm` already follow for
    /// every other realm-owning drop in this module.
    ///
    /// Production caller: `dispatch_platform_realm`'s `RealmTask::
    /// ClosePresentation` handling (`runner.rs`), when the presentation
    /// being closed is the realm's only one — closing a realm's sole
    /// presentation IS closing the realm, so that dispatch handling routes
    /// here instead of ever leaving a live realm with an empty
    /// `PresentationForest`.
    pub(super) fn request_realm_uninstall(&mut self, id: RealmId) -> Option<RealmSlot> {
        if self.dispatched_realm_id.is_some() || self.iterating_all_realms {
            self.pending_realm_mutations
                .push(RealmMapMutation::Uninstall(id));
            return None;
        }
        self.apply_uninstall(id)
    }

    /// Applies every realm-map mutation deferred while a dispatch or an
    /// all-realms iteration was in flight, in request order. Called once the
    /// deferring condition clears: the tail of `dispatch_platform_realm` and
    /// the tail of `for_each_installed_realm` (both `runner.rs`), and
    /// [`Self::should_exit`] (the drain-before-decide rule).
    ///
    /// **Early-returns (drains nothing) while `dispatched_realm_id.is_some()
    /// || iterating_all_realms` is still true** — a NESTED call reached
    /// through one of those three call sites (e.g. a dispatch run from
    /// inside a `for_each_installed_realm` visit, which that function's own
    /// doc says is safe to attempt) must not apply a mutation an OUTER,
    /// still-in-flight operation deferred: draining it early would remove a
    /// realm's slot while that realm might still be the one checked out for
    /// the outer visit, and the outer visit's own restore step
    /// (`realm_slot.realm = Some(realm)` finding no slot to write into)
    /// would then silently drop a live, un-torn-down `UiRealm` instead of
    /// restoring it. All three legitimate call sites clear their OWN flag
    /// before calling this, so the guard only ever blocks a genuinely nested
    /// caller, never the outer operation whose completion is supposed to
    /// trigger the drain.
    ///
    /// Returns every removed [`RealmSlot`] — both from a deferred
    /// `Uninstall`, AND from a deferred `Install` that collided on its
    /// window id — for the caller to drop outside the live `APP_RUNTIME`
    /// borrow — see [`Self::apply_install`]'s own doc for why that
    /// discipline matters here: this method runs inside every one of its
    /// three callers' live `RefCell` borrow, so it must never drop a
    /// realm-owning value itself.
    pub(super) fn drain_pending_realm_mutations(&mut self) -> Vec<RealmSlot> {
        if self.dispatched_realm_id.is_some() || self.iterating_all_realms {
            return Vec::new();
        }
        let pending = std::mem::take(&mut self.pending_realm_mutations);
        let mut removed = Vec::new();
        for mutation in pending {
            match mutation {
                RealmMapMutation::Install(id, slot, window) => {
                    if let Err((error, slot)) = self.apply_install(id, *slot, &window) {
                        // The collided slot is routed through `removed`, the
                        // SAME bucket a rejected/removed `Uninstall` slot
                        // uses below -- never dropped here, inside this
                        // method's live borrow.
                        tracing::error!(
                            ?id,
                            ?error,
                            "dropping a deferred realm install: its window id collided with an \
                             already-registered mapping"
                        );
                        removed.push(*slot);
                    }
                }
                RealmMapMutation::Uninstall(id) => {
                    if let Some(slot) = self.apply_uninstall(id) {
                        removed.push(slot);
                    }
                }
            }
        }
        removed
    }

    /// Teardown-only introspection: whether any realm-map mutation is still
    /// waiting to be applied. Should always be `false` by the time
    /// `teardown_platform_realm` (full loop-exit teardown) runs: both
    /// `dispatch_platform_realm` and `for_each_installed_realm` drain
    /// unconditionally in their own tails (panic or not), so nothing should
    /// ever still be queued once every in-flight dispatch/iteration has
    /// completed. `teardown_platform_realm` asserts this rather than
    /// silently dropping a still-pending mutation (and the `UiRealm` an
    /// `Install` mutation might still be holding) unnoticed.
    ///
    /// `cfg`-gated to match that one caller exactly: `teardown_platform_realm`
    /// is `#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]`
    /// (the web host never tears down at all — see that function's own
    /// module doc), so on wasm32 this method has no caller at all and must
    /// not compile there either, or it is dead code under `wasm-check`'s
    /// deny-warnings build.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn has_pending_realm_mutations(&self) -> bool {
        !self.pending_realm_mutations.is_empty()
    }

    /// Whether the loop should exit under `policy`, given the realms
    /// currently hosted, plus any [`RealmSlot`]s a deferred mutation just
    /// removed (drop these outside the `APP_RUNTIME` borrow, same as every
    /// other realm-owning drop in this module).
    ///
    /// **Drain-before-decide:** calls
    /// [`Self::drain_pending_realm_mutations`] BEFORE checking `policy` — a
    /// queued install (e.g. a splash screen's dispose callback requesting the
    /// main window) is applied first, so it vetoes exit instead of racing the
    /// empty-registry check. Without that ordering, a splash-close and a
    /// main-window-open landing in the same idle tick could observe "no
    /// realms installed" and exit before the queued install ever lands.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the exit-policy mechanism has no production embedder call site yet -- \
                      exercised by this crate's own tests"
        )
    )]
    pub(super) fn should_exit(&mut self, policy: ExitPolicy) -> (bool, Vec<RealmSlot>) {
        let removed = self.drain_pending_realm_mutations();
        let exit = match policy {
            ExitPolicy::OnLastWindowClosed => self.realms.is_empty(),
        };
        (exit, removed)
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
            runtime.realms.is_empty(),
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
                .realms
                .get(&RealmId::new_gen(0, NonZeroU32::new(1).unwrap()))
                .is_none(),
            "the RealmId-keyed registry must return None when no realm is installed"
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

    /// `ensure_services` must actually populate both `SharedEngineServices`
    /// fields with live, usable handles -- reading each one here (rather
    /// than only asserting the struct compiles) is what proves the
    /// resolution seam works, not just that it type-checks. There is no
    /// `scheduler` field to read any more: each realm now owns its own
    /// `Scheduler` (see `installed_realm_phase_tests`, below).
    #[test]
    fn ensure_services_resolves_both_and_caches_them() {
        let mut runtime = AppRuntime::new();

        let services = runtime.ensure_services();
        let _painting_image_cache = services.painting.image_cache();
        let _accessibility_features = services.accessibility_features();

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

    /// `AppRuntime::frame_wake_callback()` must be usable as the install-time
    /// `Send + Sync` handle wired onto a scheduler's `on_frame_scheduled`
    /// hook (what `install_platform_realm` now does against the just-installed
    /// realm's own `Scheduler` — see that function's doc), never a callback
    /// that re-resolves this thread-local `AppRuntime` when the hook fires.
    ///
    /// Proof shape: wire the handle onto a scheduler built right here (a
    /// stand-in for a realm's own), spawn a task on it, capture its `Waker`,
    /// then fire that `Waker` from an OS thread that never touches `runtime`
    /// or `APP_RUNTIME` at all — and observe `needs_redraw` flip on the
    /// ORIGINAL runtime anyway. A hook built by re-resolving `APP_RUNTIME` at
    /// fire time instead of capturing this `Send` handle would see an empty
    /// thread-local on the foreign thread and never flip this flag — the
    /// revert recipe for this test. (A real instance of exactly this mistake
    /// shipped once, in the frames-reenable-redirty logic — see
    /// `emit_lifecycle_transition`'s doc in `runner.rs` for that story and
    /// its fix.)
    #[test]
    fn frame_wake_callback_survives_a_cross_thread_fire_once_wired_to_a_scheduler() {
        use std::sync::mpsc;
        use std::task::Waker;
        use std::time::Duration;

        let runtime = AppRuntime::new();
        assert!(!runtime.needs_redraw(), "precondition: no redraw pending");

        // Stand-in for a realm's own scheduler; `install_platform_realm`
        // wires this exact handle onto `realm.scheduler()` at install time.
        let scheduler = flui_scheduler::Scheduler::new();
        scheduler.set_on_frame_scheduled(Some(runtime.frame_wake_callback()));

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
            // `runtime`, no `APP_RUNTIME`, no thread-local scheduler lookup
            // on this thread.
            waker.wake();
            let _ = fired_tx.send(());
        });
        fired_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the foreign thread must be able to fire the waker without blocking");

        assert!(
            runtime.needs_redraw(),
            "the wake hook wired onto the scheduler must be a Send handle captured \
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
