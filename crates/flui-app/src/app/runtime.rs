//! `AppRuntime` — the loop-scoped composition root.
//!
//! # Thesis
//!
//! This module creates the composition root and the constructor-injection
//! seams while every service it names is still singleton-*backed*. The
//! honest claim: `UiRuntime` performs zero `::instance()` calls; the services
//! it consumes are resolved once, here, in [`SharedEngineServices::resolve`].
//! Other ambient reaches (`renderer_binding.rs`, `binding.rs`,
//! `hot_reload.rs`, `config.rs`) are untouched — they remain until the
//! change that retires each singleton they reach for.
//! `flui-engine/src/wgpu/text.rs`'s ambient reach for painting has since
//! closed: the engine's glyph atlas owns a `SwashRasterizer` over the faces
//! each paragraph carries, instead of calling `PaintingBinding::instance()`. This
//! is not a forwarding shim: no old API is preserved-but-deprecated here,
//! and no ambient access point this change does not touch is claimed as
//! closed.
//!
//! # What lives here vs. in `runner.rs`
//!
//! `AppRuntime` absorbs the transitional `RuntimeHost`'s fields (UI runtime slot,
//! queue, draining flag, owner thread, address cache, window registry,
//! surface applier) plus the loop-scoped
//! `OwnerPlatform` capability (formerly a second, separate thread-local) and
//! [`SharedEngineServices`]. The single-threaded dispatch machinery that
//! operates on this struct — `install_platform_ui_runtime`,
//! `dispatch_platform_ui_runtime`, `teardown_platform_ui_runtime`,
//! `install_owner_platform`, `with_owner_platform`, the TLS declaration
//! itself — stays in `runner.rs`, unchanged in behavior: this change moves
//! *ownership* (one struct, one thread-local slot instead of two), not the
//! dispatch/teardown semantics those functions implement.

use std::sync::atomic::Ordering;

use flui_foundation::{PresentationId, UiRuntimeId};
use flui_scheduler::UpdateScheduler;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::ThreadId;

use flui_foundation::PresentationAddress;
use flui_painting::{FontCollection, HostFontFeed};
use flui_platform::OwnerPlatform;
#[cfg(target_os = "android")]
use flui_platform::traits::WindowExecutionState;
use flui_platform::traits::{Clipboard, PlatformWindow};
use flui_semantics::AccessibilityFeatures;
use parking_lot::{Mutex, RwLock};

#[cfg(not(target_arch = "wasm32"))]
use super::lifecycle::{
    ServiceDefinition, ServiceRegistry, ServiceShutdownReport, ServiceStartError,
};
use super::runner::{FontRegistrationError, PresentationDispatcher, RuntimeTask, SurfaceApplier};
use super::ui_runtime::UiRuntime;
use super::window_registry::{PreparedWindowRegistration, RegistryError, WindowRegistry};
#[cfg(not(target_arch = "wasm32"))]
use flui_runtime::execution::SpawnError;
use flui_runtime::execution::{ExecutionServices, HostExecutors};

/// Process-level engine services, each resolved **once** per owner thread in
/// [`SharedEngineServices::resolve`] — never re-resolved on every access, and
/// never reached ambiently from inside `UiRuntime`.
///
/// Owns the process-level accessibility flags and the app's one
/// [`FontCollection`] (ADR-0092 §2), which every UI runtime builds its own
/// `TextContext` from. The collection holds the bundled faces from the
/// start; the host's are added off the owner thread by the feed it was
/// built with ([`FontCollection::with_host_feed`], ADR-0092 §7), so text
/// measures, paints and places its carets in the host's faces wherever the
/// bundled ones lack a family once that feed lands. "Per owner thread" is per app while
/// ADR-0091 fixes one owner thread per process. Semantics state belongs
/// to each presentation's `SemanticsHost`; scheduling belongs to each UI runtime
/// (see `flui_runtime`'s `RuntimeServices::construct`). The retired `SemanticsBinding`
/// singleton no longer exists at all (its enablement/announce/event state
/// moved to the per-presentation `SemanticsHost` instead — see
/// `flui_runtime::semantics_host` — since that half of the old binding was a
/// per-window platform seam, not process-global state); only the OS-level,
/// read-mostly accessibility flags stayed process-scoped, and this struct
/// now owns that value directly. There is no `scheduler` field here any
/// more: each UI runtime now owns its own `UpdateScheduler` strong root (see
/// `RuntimeServices::construct` in `flui_runtime`), so there is no process-level scheduler
/// left for this struct to resolve.
pub(crate) struct SharedEngineServices {
    /// OS-level accessibility flags (reduced motion, high contrast, ...).
    /// Process-scoped and read-mostly — re-homed here from the retired
    /// `SemanticsBinding` singleton (see this struct's own doc comment).
    #[expect(
        dead_code,
        reason = "this change only creates the resolution seam; a later \
                      change wires the first real consumer"
    )]
    pub(super) accessibility_features: RwLock<AccessibilityFeatures>,
    /// The app's font collection, fed from one host scan. Every UI runtime
    /// built on this thread gets a clone (`UiRuntime::new`'s `fonts`) and owns
    /// a `TextContext` over it, so a face registered here reaches every
    /// UI runtime, and the host's faces are read once per app, not per UI runtime.
    pub(super) fonts: FontCollection,
    /// The feed that adds the host's faces to `fonts`, until
    /// [`AppRuntime::resolved_services`] launches it.
    host_feed: Cell<Option<HostFontFeed>>,
}

impl SharedEngineServices {
    /// Resolves the process-level services that survive a scheduler that is
    /// no longer process-global. Called once per owner thread
    /// (`ensure_services`'s `OnceCell::get_or_init`), the same steal-proof,
    /// idempotent guarantee the retired `AppBinding::instance()`'s
    /// thread-local initializer gave.
    fn resolve() -> Self {
        // Reached only through `AppRuntime::ensure_services()` and
        // `AppRuntime::font_collection()`, just before the first ui_runtime is
        // built. The collection holds the bundled faces now; the host scan
        // and the feed run later, off the owner thread
        // (`AppRuntime::resolved_services` launches them), and the faces they
        // add are announced like a registration's: the collection's
        // generation rises once, and every ui_runtime lays out again the text it
        // measured before. The scan is a value the feed drops once it has
        // fed the collection; nothing process-global keeps it.
        let (fonts, feed) = FontCollection::with_host_feed();

        Self {
            accessibility_features: RwLock::new(AccessibilityFeatures::default()),
            fonts,
            host_feed: Cell::new(Some(feed)),
        }
    }
}

/// What identifies a registered font's bytes: a hash of them and their
/// length.
type FontDigest = (u64, usize);

/// The fonts [`AppRuntime::register_font`] accepted on this thread.
#[derive(Default)]
struct FontRegistrations {
    /// Every accepted font, by digest, so the same bytes are never added
    /// twice.
    digests: HashSet<FontDigest>,
    /// Fonts accepted before this thread resolved its services, registered
    /// on the collection when it is built. On a thread that never builds a
    /// UI runtime they stay here, and no collection gains them.
    pending: Vec<Vec<u8>>,
}

fn font_digest(font_bytes: &[u8]) -> FontDigest {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    font_bytes.hash(&mut hasher);
    (hasher.finish(), font_bytes.len())
}

// ============================================================================
// Multi-ui_runtime hosting (issue #555)
// ============================================================================

/// One UI runtime's dispatch-adjacent state, keyed by [`UiRuntimeId`] in
/// [`AppRuntime`]'s [`RuntimeRegistry`]. Bundles exactly what the single-slot
/// design this replaces used to keep as flat `AppRuntime` fields (`ui_runtime`,
/// `queue`, `draining`, `address`, `surface_applier`): each hosted UI runtime now
/// owns its own copy of all five, so a second UI runtime's dispatch, resize
/// routing, and reentrancy guard are independent of the first's — the
/// concrete mechanism behind the end-state invariant that two windows share
/// no mutable UI tree through `AppRuntime` (sharing happens only through
/// explicit `SharedEngineServices`/app-model injection, never through this
/// registry).
pub(super) struct RuntimeSlot {
    /// `None` while this UI runtime is checked OUT of the registry for
    /// [`dispatch_platform_ui_runtime`](super::runner) — the other four fields
    /// stay in place throughout that checkout (never removed alongside
    /// `ui_runtime`), so a nested same-UI runtime dispatch still finds its target and
    /// enqueues into `queue`, instead of reading back `StaleRuntime`.
    pub(super) ui_runtime: Option<UiRuntime>,
    /// This UI runtime's own owner-thread work queue — never shared with a
    /// sibling UI runtime's queue, so draining one UI runtime's events can never pop a
    /// task meant for another. Each entry is stamped with the
    /// [`PresentationId`] of the `PresentationDispatcher` (`runner.rs`, private to
    /// that module) that enqueued it (issue #555's addressed-routing slice): a
    /// UI runtime's queue is shared across every presentation it hosts, so which
    /// window produced a given `RuntimeTask::Event` cannot be recovered from
    /// the queue's OWN dispatch call alone once more than one presentation's
    /// window can enqueue into it — the stamp travels with the task itself
    /// instead. Pump tasks are UI runtime-wide; `ClosePresentation` already
    /// carries its target id. Only `RuntimeTask::Event` reads the stamp in
    /// `RuntimeEvent::run`.
    pub(super) queue: VecDeque<(PresentationId, RuntimeTask)>,
    /// Set while a queued task for THIS UI runtime is running, so a reentrant
    /// same-UI runtime dispatch enqueues instead of recursing into `ui_runtime.take()`.
    pub(super) draining: bool,
    /// This UI runtime's routable address — the per-slot replacement for the
    /// single `AppRuntime.address: Option<PresentationAddress>` this type
    /// used to be.
    pub(super) address: PresentationAddress,
    /// This UI runtime's own registration-lifetime renderer-surface applier for a
    /// `Resized` event — never shared with a sibling UI runtime's applier, so a
    /// resize addressed to one window can never resize another's surface.
    pub(super) surface_applier: Option<SurfaceApplier>,
    /// The presentation whose window owns the surface `surface_applier`
    /// resizes: the UI runtime's primary when the applier was installed. A
    /// `Resized` for any other presentation of this UI runtime (a
    /// `WindowPolicy::Shared` secondary) must not reach that surface until
    /// sinks are per-presentation (#559).
    pub(super) surface_owner: Option<PresentationId>,
}

/// The [`UiRuntimeId`]-keyed, insertion-ordered UI runtime registry `AppRuntime` hosts
/// — the multi-UI runtime replacement for the single `Option<UiRuntime>` slot (plus
/// its four sibling flat fields) this type used to be.
///
/// Insertion order matters: hot-restart's [`super::runner`]
/// `for_each_installed_ui_runtime` and the exit-policy drain both visit UI runtimes in
/// the order they were installed, i.e. mount order.
///
/// Storage is a linear-scan `Vec`, not a hash map: the number of live UI runtimes
/// is the number of open top-level windows, small enough that O(n) lookup is
/// not a real cost, and a `Vec` gives insertion order for free with no extra
/// bookkeeping — the same reasoning `WindowRegistry` already applies to its
/// own `Vec<(WindowId, PresentationAddress)>` storage.
#[derive(Default)]
pub(super) struct RuntimeRegistry {
    slots: Vec<(UiRuntimeId, RuntimeSlot)>,
}

impl RuntimeRegistry {
    pub(super) const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Read a slot without changing its checkout state. Used for scheduler
    /// snapshots and capturing presentation assembly capabilities.
    pub(super) fn get(&self, id: &UiRuntimeId) -> Option<&RuntimeSlot> {
        self.slots
            .iter()
            .find(|(slot_id, _)| slot_id == id)
            .map(|(_, slot)| slot)
    }

    pub(super) fn get_mut(&mut self, id: &UiRuntimeId) -> Option<&mut RuntimeSlot> {
        self.slots
            .iter_mut()
            .find(|(slot_id, _)| slot_id == id)
            .map(|(_, slot)| slot)
    }

    pub(super) fn contains_key(&self, id: &UiRuntimeId) -> bool {
        self.slots.iter().any(|(slot_id, _)| slot_id == id)
    }

    /// Inserts `slot` at `id`, appending at the end of insertion order.
    ///
    /// `id` must not already be present: `next_identity` never repeats a
    /// `UiRuntimeId`, and every removal path (`remove`/`clear`) drops the old
    /// entry before a replacement is ever inserted, so a collision here
    /// means a stale id was reused — a bug this debug_assert catches instead
    /// of silently shadowing a live ui_runtime.
    pub(super) fn insert(&mut self, id: UiRuntimeId, slot: RuntimeSlot) {
        debug_assert!(
            !self.contains_key(&id),
            "BUG: RuntimeRegistry::insert called for an id already present -- \
             next_identity never repeats a UiRuntimeId, so this means a stale id was reused"
        );
        self.slots.push((id, slot));
    }

    /// Removes and returns the slot at `id`, if present.
    pub(super) fn remove(&mut self, id: &UiRuntimeId) -> Option<RuntimeSlot> {
        let index = self.slots.iter().position(|(slot_id, _)| slot_id == id)?;
        Some(self.slots.remove(index).1)
    }

    /// Removes and returns every slot, in insertion order — the
    /// multi-UI runtime generalization of `mem::take`-ing the old single
    /// `Option` slot (`install_platform_ui_runtime`'s reinstall-without-teardown
    /// path, and `teardown_platform_ui_runtime`'s full loop-exit teardown, both
    /// use this).
    pub(super) fn clear(&mut self) -> Vec<(UiRuntimeId, RuntimeSlot)> {
        std::mem::take(&mut self.slots)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &(UiRuntimeId, RuntimeSlot)> {
        self.slots.iter()
    }

    /// Every installed `UiRuntimeId`, in insertion (mount) order — the read
    /// `for_each_installed_ui_runtime` (`super::runner`) snapshots before it
    /// starts checking UI runtimes out one at a time.
    #[cfg(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    ))]
    pub(super) fn keys(&self) -> Vec<UiRuntimeId> {
        self.slots.iter().map(|(id, _)| *id).collect()
    }
}

/// A UI runtime-registry install/uninstall requested while a mutation must defer
/// (a dispatch or an all-UI runtimes iteration is in flight on this thread — see
/// [`AppRuntime::request_ui_runtime_install`]/[`AppRuntime::request_ui_runtime_uninstall`]).
/// Applied in request order once the deferring condition clears. Private:
/// external callers (`super::runner`) go through the typed
/// `request_ui_runtime_install`/`request_ui_runtime_uninstall` methods, never
/// construct this enum directly — keeping the window-registration step
/// (see [`AppRuntime::apply_install`]) bundled with the registry insert
/// atomically, instead of requiring every call site to remember both.
enum RuntimeMapMutation {
    /// Add a newly-constructed UI runtime to the registry (never displaces a
    /// sibling — see `install_ui_runtime_alongside` in `super::runner`), plus the
    /// prepared native identity for its `WindowRegistry` mapping. Boxed: `RuntimeSlot`
    /// owns a whole `UiRuntime`, over a kilobyte, next to `Uninstall`'s bare
    /// `UiRuntimeId` -- boxing keeps this enum (and every `Vec<RuntimeMapMutation>`
    /// queueing it) from paying that size for every entry regardless of
    /// variant.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "constructed only by request_ui_runtime_install, whose one production caller \
                      (runner.rs::install_ui_runtime_alongside) is desktop-only"
        )
    )]
    Install(UiRuntimeId, Box<RuntimeSlot>, PreparedWindowRegistration),
    /// Remove one UI runtime from the registry (a window closing while siblings
    /// stay open — see `request_ui_runtime_uninstall` in `super::runner`).
    Uninstall(UiRuntimeId),
}

/// Governs when the platform loop should exit once every hosted UI runtime's
/// window has closed — the embedder-facing policy knob for the "new
/// independent desktop window ⇒ new UI runtime" production policy (ADR-0027,
/// issue #555).
///
/// Consulted through `AppRuntime::should_exit`, which drains any deferred
/// UI runtime-map mutation FIRST (the drain-before-decide rule): a
/// queued "open another window" install — e.g. a splash screen's dispose
/// callback requesting the main window — is applied before the
/// empty-registry check, so it vetoes exit instead of racing it. Without
/// that ordering, a splash-close and a main-window-open landing in the same
/// idle tick could observe "no UI runtimes installed" and exit before the queued
/// install ever lands.
///
/// `#[non_exhaustive]`: a future variant (e.g. "never exit automatically;
/// the embedder calls `quit()` itself") must not be a breaking change for a
/// caller that already matches exhaustively — the same precedent
/// `AttachError` set (`binding.rs`).
///
/// **Live-wired** (issue #555's native-lifecycle follow-up): `AppConfig::
/// exit_policy` carries this to `run_desktop`'s bootstrap, which installs a
/// hook (`runner.rs`'s `install_exit_policy_hook`) into
/// `flui_platform::traits::Platform::set_exit_policy_hook`. The winit
/// backend's `CloseRequested` handling and the headless backend's window
/// `close()`/`simulate_close()` both consult that hook — via the shared
/// `flui_platform::shared::PlatformHandlers::exit_policy` slot — instead of
/// deciding from their own native window count alone; see
/// `closing_the_last_window_reentrantly_from_inside_a_dispatch_still_exits`
/// (`owner_dispatch/tests.rs`), which closes the last window through the
/// real platform hook. Android/web bootstraps do not install this hook
/// today (their platforms don't override `set_exit_policy_hook` either, so
/// doing so would be inert) — stated, not silently assumed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitPolicy {
    /// Exit once every hosted UI runtime has closed and none is queued to open.
    /// The default policy.
    #[default]
    OnLastWindowClosed,
    /// Remain alive without windows until explicitly asked to quit.
    ExplicitQuit,
}

/// What a second top-level window shares with the windows already open on
/// this thread. Chosen when the window opens
/// ([`super::runner::open_secondary_window`]); nothing in its widget tree or
/// its `BuildContext` names the policy afterwards.
///
/// FLUI calls the unit a window belongs to a *UI runtime*: its own widget state,
/// `GlobalKey` scope and frame scheduler.
///
/// The choice decides whether the two windows can disturb each other:
///
/// - [`WindowPolicy::Isolated`] (the default): the new window gets its own
///   state, keys and scheduler, as if it were a separate application that
///   shares only the GPU and font services. The same `GlobalKey` can be
///   mounted in both windows, and a slow frame in one never delays the other.
/// - [`WindowPolicy::Shared`]: the new window is a second view of the same
///   session — for example a detached inspector panel. Both windows share one
///   `GlobalKey` scope, so mounting the same key in both fails at the second
///   mount (ADR-0043), and one scheduler and async driver, so a slow frame in
///   either window can delay the other's next frame.
///
/// `#[non_exhaustive]`: a future policy (joining a named session, say) must
/// not break an exhaustive match.
#[doc(alias = "ui_runtime")]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
// iOS is the only target where this is genuinely dead: its public re-export
// is gated out there (see `lib.rs`), while android and wasm32 keep the
// re-export and therefore a reachable path.
#[cfg_attr(
    all(not(test), target_os = "ios"),
    expect(
        dead_code,
        reason = "secondary-window policy; its iOS re-export is gated out"
    )
)]
pub enum WindowPolicy {
    /// The new window gets its own state, `GlobalKey` scope and scheduler,
    /// sharing only the GPU and font services. The default: a slow window can
    /// never delay another's frames.
    #[doc(alias = "SeparateRuntimes")]
    #[default]
    Isolated,
    /// The new window is a second window of the same session: it shares
    /// widget state, the `GlobalKey` scope and the scheduler with the first
    /// window opened on this thread.
    #[doc(alias = "SharedRuntime")]
    Shared,
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
#[derive(Clone)]
struct FrameWakeHandle {
    needs_redraw: Arc<AtomicBool>,
    redraw_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>>,
}

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

/// Owner-loop quit notification progress; reset only when installing a new owner.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuitNotification {
    Active,
    Requested,
    Notifying,
    Notified,
}

/// The loop-scoped composition root: platform event-loop demux, the single
/// UI runtime slot, and the once-resolved [`SharedEngineServices`].
///
/// Absorbs the former `RuntimeHost` (UI runtime slot, queue, draining flag, owner
/// thread, address cache, window registry, surface applier)
/// wholesale, plus the loop-scoped `OwnerPlatform` capability (formerly the
/// separate `OWNER_PLATFORM_HOST` thread-local) and `services`. One struct,
/// one thread-local slot (`runner.rs`'s `APP_RUNTIME`) — the same two
/// invariants that justified two separate TLS cells before still hold as two
/// fields on one struct: `teardown_platform_ui_runtime` clears the UI runtime-facing
/// fields and *never* `owner_platform` (hot-restart hosts a fresh UI runtime on
/// the same loop); `OwnerHostClearGuard` clears only `owner_platform` on
/// unwind.
///
/// # Design-for-N
///
/// The UI runtime-facing API is `UiRuntimeId`-keyed: `ui_runtimes` is [`RuntimeRegistry`],
/// an insertion-ordered map of any number of hosted UI runtimes (issue #555) —
/// the UI runtime's `next_identity` (`flui_runtime`) already mints from a shape that never needed to
/// change for this to land.
pub(crate) struct AppRuntime {
    /// Native frame resources; logical runtime membership stays independent.
    pub(super) frame_drivers: super::runner::FrameDrivers,
    pub(super) native_retirement: super::runner::NativeRetirement,
    /// Every hosted UI runtime, keyed by `UiRuntimeId`, in mount (insertion) order.
    /// Replaces the single `Option<UiRuntime>` slot (plus its four sibling
    /// flat fields `queue`/`draining`/`address`/`surface_applier`) this
    /// struct used to carry — see [`RuntimeSlot`]'s doc for why those four
    /// moved inside the per-UI runtime entry instead of staying flat.
    pub(super) ui_runtimes: RuntimeRegistry,
    /// Owner-local work accepted while another UI runtime callback is running.
    ///
    /// This is one queue for the whole loop, rather than one queue per UI runtime:
    /// reentrant A -> B -> A dispatch must run after the current callback in
    /// exactly that admission order. Entries retain their complete stamped
    /// dispatcher so stale ui_runtime/presentation admission is checked again
    /// when the turn reaches the front of the queue.
    pub(super) owner_turn_queue: VecDeque<(PresentationDispatcher, RuntimeTask)>,
    /// True while the outermost [`dispatch_platform_ui_runtime`](super::runner)
    /// invocation owns the queue drain. Reentrant dispatch only appends and
    /// returns; it never recursively checks out a sibling UI runtime.
    pub(super) owner_turn_draining: bool,
    /// Sequence of the one continuation opportunity currently requested for
    /// carried owner work. The sequence lets a failed, synchronously
    /// reentrant actuator clear only its own reservation.
    pub(super) owner_turn_continuation: Option<u64>,
    /// The last attempt to post a continuation failed. A fresh native root
    /// must still run synchronously instead of joining the carried backlog;
    /// its tail retries the post.
    pub(super) owner_turn_continuation_failed: bool,
    /// Remaining logical operations in the native callback that consumed a
    /// continuation. `None` means this callback is not servicing carried
    /// owner work.
    pub(super) owner_turn_callback_budget: Option<usize>,
    /// True from the outermost native callback entry until its completion
    /// work has finished. Some platform adapters (notably the web window)
    /// can synchronously invoke a frame callback from `request_redraw`, so a
    /// nested entry is another root of the current physical callback, not a
    /// second opportunity that may consume or finish its budget.
    pub(super) owner_turn_callback_active: bool,
    /// Monotonic source for [`Self::owner_turn_continuation`].
    pub(super) owner_turn_next_sequence: u64,
    /// Host-specific continuation actuator. It is owner-local because
    /// `AppRuntime` is owner-affine; `true` means an opportunity was posted.
    pub(super) owner_turn_wake: Option<Rc<dyn Fn() -> bool>>,
    /// Addresses whose terminal close has been admitted but may still be
    /// waiting in a bounded batch. Later work cannot jump that barrier.
    pub(super) closing_presentations: HashSet<PresentationAddress>,
    /// The thread that installed the first UI runtime hosted here; every dispatch
    /// checks against this before touching the registry. Loop-scoped, not
    /// per-UI runtime: every UI runtime this `AppRuntime` ever hosts lives on the same
    /// owner thread (`APP_RUNTIME` is thread-local), so one shared value is
    /// exact, not an approximation of a per-UI runtime concept.
    pub(super) owner_thread: Option<ThreadId>,
    /// The sole native-window-to-presentation mapping authority (ADR-0037
    /// §2), already `UiRuntimeId`-keyed and multi-window-shaped — see its own
    /// module doc.
    pub(super) registry: WindowRegistry,
    /// Per-presentation close-request handlers (issue #558) — the seam by
    /// which an application vetoes a window close. `Arc`, not inline: each
    /// window's own `on_should_close` closure holds a clone and answers
    /// without re-entering this thread-local at all, which is what lets it
    /// answer while a UI runtime is checked out for dispatch. See
    /// [`CloseRequestRouter`](super::close_request::CloseRequestRouter)'s
    /// own doc.
    close_requests: Arc<super::close_request::CloseRequestRouter>,
    /// The loop-scoped owner-thread platform capability (ADR-0039 §6).
    /// Deliberately *not* cleared by UI runtime teardown — the loop may host
    /// another UI runtime before it exits (hot-restart does exactly this).
    pub(super) owner_platform: Option<std::rc::Rc<OwnerPlatform>>,
    pub(super) owner_install_generation: u64,
    /// A clone of the currently-dispatched UI runtime's scheduler, held ONLY
    /// while `dispatch_platform_ui_runtime` (in `runner.rs`) has taken that
    /// UI runtime's slot out of `ui_runtimes` above for the duration of a queued task.
    /// Without this, [`Self::installed_ui_runtime_phase`] (`with_owner_platform`'s
    /// fence (c)) reads `None` for the UI runtime's entire dispatched extent —
    /// not just when no UI runtime is installed at all — because
    /// `dispatch_platform_ui_runtime` checks the UI runtime's `UiRuntime` OUT of its slot
    /// before running any task, including the frame pump that drives the
    /// scheduler through `PersistentCallbacks`. That makes the fence blind
    /// exactly when a frame phase is actually running, which is the one case
    /// the fence exists to catch. `UpdateScheduler` is a single-`Arc` handle (see
    /// `flui-scheduler`'s `UpdateScheduler`/`SchedulerInner` split), so cloning it
    /// here to survive the checkout is cheap — an `Arc::clone`, not a new
    /// scheduler. Set at checkout, cleared at restore
    /// (`dispatch_platform_ui_runtime`), in both cases inside the same
    /// `catch_unwind`-guarded block that restores the UI runtime's slot itself, so
    /// an unwinding dispatched task leaves this `None` exactly as reliably as
    /// it leaves the slot restored.
    ///
    /// Stays single-slot on purpose: dispatch is
    /// single-threaded and sequential, so at most one UI runtime is EVER checked
    /// out on this thread at a given instant — [`RuntimeMapMutation`]'s
    /// defer-to-idle discipline exists precisely so no second, nested
    /// dispatch for a DIFFERENT UI runtime is ever attempted while this is
    /// `Some`; `dispatch_platform_ui_runtime` debug-asserts that invariant at its
    /// one checkout site.
    pub(super) dispatched_scheduler: Option<UpdateScheduler>,
    /// The identity companion to `dispatched_scheduler` above: which UI runtime
    /// is currently checked out, so a nested dispatch attempt targeting a
    /// DIFFERENT UI runtime can be told apart from a legitimate same-UI runtime
    /// reentrant call (already handled by each slot's own `draining` flag).
    /// Also set (alongside `dispatched_scheduler`) for the UI runtime
    /// `for_each_installed_ui_runtime` currently has checked out of the registry
    /// mid-visit — from fence (c)'s perspective a visited UI runtime IS
    /// dispatched: its own scheduler phase must still be observable, not
    /// blind, for the whole time it sits outside `ui_runtimes`.
    pub(super) dispatched_ui_runtime_id: Option<UiRuntimeId>,
    /// Set for the duration of `for_each_installed_ui_runtime`'s (`runner.rs`)
    /// hot-restart-shaped visit over every hosted UI runtime. A UI runtime-map
    /// mutation requested while this is `true` defers exactly like one
    /// requested while `dispatched_ui_runtime_id` is `Some` — see
    /// [`Self::request_ui_runtime_install`]/[`Self::request_ui_runtime_uninstall`] —
    /// so a mutation triggered by a callback running mid-visit never
    /// changes the set of UI runtimes that same visit is still walking.
    pub(super) iterating_all_ui_runtimes: bool,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(super) quit_notification: QuitNotification,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(super) loop_identity: Arc<()>,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(super) main_controller: Option<super::runner::main_window::MainController>,
    #[cfg(target_os = "ios")]
    pub(super) ios_controller: Option<super::runner::ios::IOSController>,
    #[cfg(target_os = "ios")]
    pub(super) ios_running: bool,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(super) main_ingress: Option<Arc<super::application_control::Ingress>>,
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(super) main_host_lifecycle: flui_scheduler::AppLifecycleState,
    /// Accepted loop-owned window requests, including currently polled/installing entries.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) pending_window_reservations: Arc<std::sync::atomic::AtomicUsize>,
    /// Runtime-map mutations requested while
    /// [`Self::request_ui_runtime_install`]/[`Self::request_ui_runtime_uninstall`]
    /// decided they must defer. Applied, in request order, by
    /// [`Self::drain_pending_ui_runtime_mutations`].
    pending_ui_runtime_mutations: Vec<RuntimeMapMutation>,
    /// Process-level engine services. Deliberately **not** resolved in
    /// [`AppRuntime::new`] -- see [`AppRuntime::ensure_services`] for why.
    services: OnceCell<SharedEngineServices>,
    /// The fonts registered through [`Self::register_font`], and those still
    /// waiting for `services`.
    fonts: RefCell<FontRegistrations>,
    /// The collection's generation the UI runtimes were last told of
    /// ([`Self::take_font_change`]).
    fonts_announced: Cell<u64>,
    /// The loop-scoped background execution services (issue #557): both
    /// background work-class lanes, their bounded admission, and the
    /// shutdown protocol. Built at UI runtime install
    /// ([`Self::ensure_execution`]) from either the host-injected
    /// [`HostExecutors`] stashed by [`Self::install_host_executors`] or the
    /// lazily-started default pools. Loop-scoped like `owner_platform`:
    /// hot-restart hosts a fresh UI runtime on the same loop and must not
    /// rebuild pools, so UI runtime teardown never touches this — only full
    /// loop-exit teardown shuts it down AND clears this slot
    /// ([`Self::shutdown_execution`]), so a later second loop on this same
    /// thread re-resolves fresh services instead of inheriting a dead
    /// instance.
    /// `Arc`ed (issue #558) so the lifecycle layer can hold the services
    /// weakly: a `TaskSpawner` that outlives this loop refuses with
    /// `SpawnError::ShuttingDown` when its upgrade fails, instead of
    /// keeping dead pools alive or reaching ambient state.
    execution: OnceCell<Arc<ExecutionServices>>,
    /// The running application services (issue #558): the named owner of
    /// every application-lifetime unit of background work. Loop-scoped for
    /// the same hot-restart reason as `execution` above; consulted by
    /// [`Self::should_exit`] (a running keep-alive service vetoes exit)
    /// and shut down — cancel, then deadline-bounded join — by
    /// [`Self::shutdown_lifecycles`] at full loop-exit teardown, BEFORE
    /// the pools close.
    #[cfg(not(target_arch = "wasm32"))]
    service_registry: ServiceRegistry,
    /// Host executors received from `AppConfig` before the first UI runtime
    /// install resolves `execution`. Taken by [`Self::ensure_execution`];
    /// ignored (with a warning) if execution services already exist.
    pending_host_executors: Option<HostExecutors>,
    /// Whether a redraw has been requested since the last
    /// `mark_rendered` — the loop-scoped half of the retired
    /// `AppBinding.needs_redraw` flag, re-homed here as part of `AppBinding`'s
    /// dissolution. Loop-scoped, not UI runtime-scoped: a hot-restart that tears
    /// down and reinstalls a UI runtime on this same thread must not lose a
    /// pending redraw request, and [`Self::frame_wake_callback`] hands a
    /// clone of this exact `Arc` to callbacks that may fire from any thread.
    needs_redraw: Arc<AtomicBool>,
    /// The window [`Self::wake_frame`] pokes via
    /// [`PlatformWindow::request_redraw`], installed by
    /// [`Self::set_redraw_window`] once the UI runtime's window is open and
    /// cleared at teardown. Distinct from `PresentationState.window`
    /// (per-presentation, `Weak`, used for cursor/haptics/close): this slot
    /// is `Arc`-strong and `Send + Sync` specifically so
    /// [`Self::frame_wake_callback`] can hand a cross-thread-safe clone to a
    /// callback that fires off the owner thread, which a `Weak` field owned
    /// by a `!Send` `UiRuntime` cannot support.
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
    /// The byte storage every UI runtime this host builds hands its widgets,
    /// resolved once from the run's configuration when the host starts
    /// ([`Self::install_host_storage`]) and released with the owner platform
    /// at loop exit.
    pub(super) host_storage: Option<Arc<dyn flui_platform_api::Storage>>,
}

impl AppRuntime {
    /// Construct the composition root: cheap, side-effect-free.
    /// Called as the `APP_RUNTIME` TLS slot's own initializer (`runner.rs`),
    /// so simply *touching* the thread-local -- for any reason, on any
    /// thread -- can never itself run singleton construction or full
    /// system-font enumeration. Real service resolution happens only when a
    /// UI runtime is built or installed -- [`Self::font_collection`] for
    /// `UiRuntime::new`, or the explicit [`Self::ensure_services`] call from
    /// `install_platform_ui_runtime` -- never from an incidental
    /// first touch such as `OwnerHostClearGuard::drop` unwinding through a
    /// virgin thread, and never from `install_owner_platform` either (every
    /// backend calls that, including `run_direct`, which never installs a
    /// UI runtime and never needs these services).
    ///
    /// Not `const fn`: the wake/clipboard fields below need their own
    /// independent `Arc` allocations (three small ones), which the allocator
    /// makes non-const-evaluable. That is a cheap, ordinary heap allocation,
    /// not the singleton construction or system-font enumeration this
    /// function's side-effect-free contract is actually about — `services`
    /// (the `OnceCell`) staying unresolved is what that contract depends on,
    /// and this change does not touch it.
    pub(super) fn new() -> Self {
        let native_retirement = super::runner::NativeRetirement::default();
        Self {
            ui_runtimes: RuntimeRegistry::new(),
            frame_drivers: super::runner::FrameDrivers::new(native_retirement.clone()),
            native_retirement,
            owner_turn_queue: VecDeque::new(),
            owner_turn_draining: false,
            owner_turn_continuation: None,
            owner_turn_continuation_failed: false,
            owner_turn_callback_budget: None,
            owner_turn_callback_active: false,
            owner_turn_next_sequence: 0,
            owner_turn_wake: None,
            closing_presentations: HashSet::new(),
            owner_thread: None,
            registry: WindowRegistry::new(),
            close_requests: Arc::new(super::close_request::CloseRequestRouter::new()),
            owner_platform: None,
            owner_install_generation: 0,
            dispatched_scheduler: None,
            dispatched_ui_runtime_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_window_reservations: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            iterating_all_ui_runtimes: false,
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            quit_notification: QuitNotification::Active,
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            loop_identity: Arc::new(()),
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            main_controller: None,
            #[cfg(target_os = "ios")]
            ios_controller: None,
            #[cfg(target_os = "ios")]
            ios_running: false,
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            main_ingress: None,
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            main_host_lifecycle: flui_scheduler::AppLifecycleState::Detached,
            pending_ui_runtime_mutations: Vec::new(),
            services: OnceCell::new(),
            fonts: RefCell::new(FontRegistrations::default()),
            fonts_announced: Cell::new(0),
            execution: OnceCell::new(),
            #[cfg(not(target_arch = "wasm32"))]
            service_registry: ServiceRegistry::new(),
            pending_host_executors: None,
            needs_redraw: Arc::new(AtomicBool::new(false)),
            redraw_window: Arc::new(Mutex::new(None)),
            platform_clipboard: Arc::new(Mutex::new(None)),
            host_storage: None,
        }
    }

    /// Resolves and caches [`SharedEngineServices`] on first call; returns
    /// the cached value on every later call. Called from
    /// `install_platform_ui_runtime`, when a UI runtime is actually about to be
    /// installed on this thread; [`Self::font_collection`] resolves the same
    /// cell a step earlier, when the runner builds that UI runtime --
    /// `install_owner_platform` deliberately
    /// does NOT call this (see its own doc): every backend calls that,
    /// including `run_direct`, which opens a window but never installs a
    /// UI runtime and never consumes painting/semantics/scheduler services, so
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
    /// explicit call restores the old `RuntimeHost`-era guarantee that merely
    /// touching the thread-local is always safe to do from within a
    /// clear-guard drop.
    pub(super) fn ensure_services(&mut self) -> &SharedEngineServices {
        self.resolved_services()
    }

    /// The services, resolved on first call, with every font registered
    /// before then added to their collection: a UI runtime built over the
    /// collection measures with those faces from its first frame.
    ///
    /// The first call also launches the host font feed, after those fonts:
    /// a family the app registered before the start is then already held,
    /// and the feed adds no host copy of it. The generation the UI runtimes know
    /// is taken before the launch, so the feed's landing is announced
    /// ([`Self::take_font_change`]) even if it lands at once.
    fn resolved_services(&self) -> &SharedEngineServices {
        let services = self.services.get_or_init(SharedEngineServices::resolve);
        let pending = std::mem::take(&mut self.fonts.borrow_mut().pending);
        for font_bytes in pending {
            // Checked when it was accepted; a refusal now would be the two
            // sides disagreeing with the check, and refuses on both.
            if let Err(error) = services.fonts.register_font(&font_bytes) {
                tracing::warn!(%error, "a font registered before the first ui_runtime was refused");
            }
        }
        if let Some(feed) = services.host_feed.take() {
            self.fonts_announced.set(services.fonts.generation());
            LAUNCH_HOST_FEED(feed, self.frame_wake_callback());
        }
        services
    }

    /// Whether the app's font collection changed since the UI runtimes were last
    /// told, and records that they are told now.
    ///
    /// A change is a registration or the host feed landing; either raises
    /// the collection's generation. `false` until the services resolve. One
    /// atomic load when nothing changed.
    pub(super) fn take_font_change(&self) -> bool {
        let Some(services) = self.services.get() else {
            return false;
        };
        let now = services.fonts.generation();
        self.fonts_announced.replace(now) != now
    }

    /// The app's font collection, for `UiRuntime::new`'s `fonts`: a clone of
    /// the one [`SharedEngineServices`] owns, resolving the services first if
    /// no UI runtime has been built yet. Every call returns the same collection.
    ///
    /// Takes `&self` (`OnceCell::get_or_init` needs no more), so a runner
    /// reaches it through the same shared `APP_RUNTIME` borrow as the
    /// clipboard. A panic inside the resolution leaves the cell empty and
    /// the next call retries, as [`Self::ensure_services`] does.
    pub(super) fn font_collection(&self) -> FontCollection {
        self.resolved_services().fonts.clone()
    }

    /// Registers `font_bytes` on the app's font collection, which measures,
    /// paints and places carets in text.
    ///
    /// Before this thread has built a UI runtime, the bytes are checked and held,
    /// and registered when the first UI runtime resolves the services: a thread
    /// that never runs the app never scans the host's fonts for them.
    ///
    /// Telling the UI runtimes is the caller's work, outside this borrow
    /// (`runner::register_font`, through [`Self::take_font_change`]).
    ///
    /// # Errors
    ///
    /// [`FontRegistrationError::AlreadyRegistered`] for bytes registered
    /// before, [`FontRegistrationError::Font`] for bytes with no face; either
    /// way nothing changes.
    pub(super) fn register_font(&self, font_bytes: &[u8]) -> Result<(), FontRegistrationError> {
        let digest = font_digest(font_bytes);
        if self.fonts.borrow().digests.contains(&digest) {
            return Err(FontRegistrationError::AlreadyRegistered);
        }
        if let Some(services) = self.services.get() {
            services.fonts.register_font(font_bytes)?;
        } else {
            FontCollection::check_font(font_bytes)?;
            self.fonts.borrow_mut().pending.push(font_bytes.to_vec());
        }
        self.fonts.borrow_mut().digests.insert(digest);
        Ok(())
    }

    /// Stash the host's executors ahead of the first UI runtime install (the
    /// bootstrap step that resolves [`Self::ensure_execution`]). Called by
    /// the runner when `AppConfig::executors` is `Some` — before
    /// `install_platform_ui_runtime`, so the resolved services route to the host
    /// instead of constructing the default pools.
    ///
    /// A stash arriving after execution services already exist is ignored
    /// with a warning rather than rebuilt: pools may already be running
    /// work, and silently swapping executors mid-run would strand it.
    // Its one production caller (bootstrap_desktop's config wiring) is
    // desktop-only; android/wasm have no host-injection entry point yet.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "host executors are injected via AppConfig on the desktop bootstrap \
                      only; android/web bootstraps gain the wiring with their own \
                      host-injection slice"
        )
    )]
    pub(super) fn install_host_executors(&mut self, host: HostExecutors) {
        if self.execution.get().is_some() {
            tracing::warn!(
                "install_host_executors called after execution services were \
                 already resolved; the injected executors are ignored"
            );
            return;
        }
        self.pending_host_executors = Some(host);
    }

    /// Resolves and caches the loop-scoped [`ExecutionServices`] on first
    /// call (host-injected if [`Self::install_host_executors`] stashed a
    /// bundle, default pools otherwise); returns the cached value on every
    /// later call. Called from `install_platform_ui_runtime` alongside
    /// [`Self::ensure_services`] — a UI runtime is actually being installed, so
    /// this loop genuinely hosts application work. Cheap either way: the
    /// default pools start worker threads lazily, on first background
    /// spawn, never here.
    pub(super) fn ensure_execution(&mut self) -> &Arc<ExecutionServices> {
        let host = self.pending_host_executors.take();
        self.execution.get_or_init(|| {
            Arc::new(match host {
                Some(host) => ExecutionServices::with_host(host),
                None => ExecutionServices::with_defaults(),
            })
        })
    }

    /// Start an application service (issue #558): resolve the loop's
    /// execution services if needed, hand the service its context, and
    /// register it with this runtime's [`ServiceRegistry`] — the named
    /// owner that will cancel and join it at loop-exit teardown, and whose
    /// running keep-alive services [`Self::should_exit`] consults.
    ///
    /// # Errors
    ///
    /// [`ServiceStartError`]: the IO lane refused the service's future,
    /// the registry has already begun shutting down, or the service's own
    /// factory panicked (contained, not unwound).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn start_service(
        &mut self,
        definition: &ServiceDefinition,
    ) -> Result<(), ServiceStartError> {
        // Admission first: a refused late start (after loop-exit teardown,
        // before any next install) must not resurrect execution services
        // as a side effect of the check.
        if !self.service_registry.is_accepting() {
            return Err(ServiceStartError::Spawn(SpawnError::ShuttingDown));
        }
        let services = Arc::clone(self.ensure_execution());
        self.service_registry.start(definition, &services)
    }

    /// Install the notifier fired when a `KeepsAppAlive` service reports
    /// its exit (issue #558) — the runner wires this to the platform's
    /// coalesced exit-policy re-evaluation request, closing the loop that
    /// [`Self::should_exit`]'s service veto opens: without it, a veto
    /// taken at the last window's close would never be re-decided once
    /// the vetoing service completes.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn set_lifecycle_exit_notifier(&mut self, notifier: Arc<dyn Fn() + Send + Sync>) {
        self.service_registry.set_exit_notifier(notifier);
    }

    /// The installed exit-policy re-evaluation notifier, if any.
    ///
    /// Installed for the keep-alive-service case, but the mechanism is not
    /// specific to it: it is the platform's coalesced, owner-thread request
    /// to consult the exit-policy hook again, and by its own contract a
    /// spurious fire is a no-op. Any caller that changes the UI runtime map at a
    /// moment the hook cannot observe needs it — see
    /// `dispatch_platform_ui_runtime`'s tail.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn exit_policy_reevaluation_notifier(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.service_registry.exit_notifier()
    }

    /// Reopen service admission for a loop that is (re)installing a UI runtime
    /// — the registry counterpart of the `execution` slot's second-loop
    /// reset. Running services are untouched: a mid-loop reinstall
    /// (hot-restart, panic recovery) finds admission already open and its
    /// services still owned; only a registry closed by a PRIOR loop's
    /// teardown observably changes state here.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn reopen_lifecycles(&mut self) {
        self.service_registry.reopen();
    }

    /// The staged service shutdown (issue #558): stop admission, cancel
    /// every running service, then join each against `deadline` — run at
    /// full loop-exit teardown BEFORE [`Self::shutdown_execution`], so
    /// services get their cooperative flush window before the pools
    /// hard-drop whatever is left. Returns the per-service join evidence.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn shutdown_lifecycles(
        &mut self,
        deadline: std::time::Duration,
    ) -> ServiceShutdownReport {
        self.service_registry.shutdown(deadline)
    }

    /// The resolved execution services, if any. `None` before the first
    /// UI runtime install and on a loop (like `run_direct`'s) that never installs
    /// a UI runtime.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production paths resolve through ensure_execution/start_service; \
                      only tests read the resolved services back through this accessor"
        )
    )]
    pub(super) fn execution(&self) -> Option<&ExecutionServices> {
        self.execution.get().map(Arc::as_ref)
    }

    /// Shut down the execution services and **reset the slot**: stop
    /// admission, cancel outstanding work, join running work bounded by
    /// `grace` per pool, then take the shut-down instance out of the
    /// `OnceCell` (dropping it — its pools are already closed, so the drop
    /// is a no-op) and discard any never-resolved host-executor stash. Runs
    /// at full loop-exit teardown (`teardown_platform_ui_runtime`), never at
    /// per-UI runtime teardown — see the `execution` field's doc.
    ///
    /// Resetting the slot (rather than leaving the dead instance in place)
    /// is load-bearing for a SECOND platform loop hosted on this same
    /// thread later — an embedder running `run_app` twice in one process,
    /// or a headless-restart harness: the next loop's UI runtime install must
    /// re-resolve a fresh, working `ExecutionServices` (honoring any newly
    /// stashed `HostExecutors`), not inherit an instance whose admission is
    /// permanently closed. Nothing rebuilds pools on the loop that is
    /// exiting either way: `ensure_execution` (the only resolver) runs only
    /// from a UI runtime install, and this loop is past its last one. A teardown
    /// path that skips this call still tears pools down non-blockingly
    /// (`ExecutionServices`' own `Drop`).
    // Its production caller (teardown_platform_ui_runtime) is not compiled on
    // wasm32, where shutdown is a no-op by construction (sequential
    // execution; nothing to join).
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "full loop-exit teardown (the caller) does not exist on wasm32; \
                      the wasm backend has nothing to cancel or join"
        )
    )]
    pub(super) fn shutdown_execution(&mut self, grace: std::time::Duration) {
        // A stash the exiting loop never resolved must not leak into the
        // next loop's resolution: injection is per-run configuration, and
        // the next `run_*` stashes its own config's bundle if it has one.
        self.pending_host_executors = None;
        if let Some(services) = self.execution.take() {
            services.shutdown(grace);
        }
    }

    /// True if `phase` is one of the phases `with_owner_platform`'s fence (c)
    /// forbids: a frame transaction genuinely in flight, as opposed to
    /// `Idle`/`PostFrameCallbacks`, both of which are legitimate times to
    /// acquire owner-platform capability (ADR-0021-style post-frame work).
    // Reachable only from `with_owner_platform`'s `#[cfg(debug_assertions)]`
    // fence, so in a release build both helpers are dead. They stay compiled
    // in both profiles (a future release-profile caller must not find them
    // missing) and the attribute states why the lint is expected there.
    #[cfg_attr(
        not(debug_assertions),
        expect(
            dead_code,
            reason = "only the debug-only owner-platform fence calls this"
        )
    )]
    fn is_frame_transaction_phase(phase: flui_scheduler::SchedulerPhase) -> bool {
        matches!(
            phase,
            flui_scheduler::SchedulerPhase::TransientCallbacks
                | flui_scheduler::SchedulerPhase::MidFrameMicrotasks
                | flui_scheduler::SchedulerPhase::PersistentCallbacks
        )
    }

    /// The installed UI runtime's scheduler phase, or `None` if no UI runtime is
    /// installed on this thread AND no UI runtime is currently checked out for
    /// dispatch either — the replacement for `with_owner_platform`'s fence
    /// (c), formerly a bare `UpdateScheduler::instance().phase()` read.
    ///
    /// **Design-for-N (issue #555): iterates every resident slot in
    /// `ui_runtimes`**, rather than assuming a single slot is "the" UI runtime that
    /// matters — but checks [`Self::dispatched_scheduler`] FIRST: dispatch is
    /// single-threaded and sequential, so whenever a UI runtime is checked out for
    /// `dispatch_platform_ui_runtime` (the entire extent of a queued task,
    /// including the frame pump that drives `PersistentCallbacks`), every
    /// OTHER resident UI runtime is necessarily `Idle`/`PostFrameCallbacks` (no
    /// two UI runtimes ever run their frame pump concurrently on one thread) —
    /// reading `dispatched_scheduler` first is therefore both sufficient and
    /// exact for that case, not merely a fallback. Only once nothing is
    /// checked out does this fall through to scanning `ui_runtimes` itself, so an
    /// addressed probe (this dispatch's own UI runtime) and the aggregate fence
    /// both stay correct without duplicating the forbidden-phase list.
    /// Single-pass, no intermediate allocation: returns the first FORBIDDEN
    /// phase found, short-circuiting the scan; if none is forbidden, returns
    /// the first UI runtime's OBSERVED phase honestly (never claimed to be
    /// specifically `Idle` — a fully quiescent UI runtime can legitimately sit in
    /// `PostFrameCallbacks` just as validly, and this reads back whichever
    /// one it actually is) rather than `None`, matching this function's
    /// pre-registry single-slot behavior for the common one-UI runtime case (a
    /// UI runtime being installed reads back its own real phase, not a
    /// vacuous "nothing installed" `None`).
    // Reachable only from `with_owner_platform`'s `#[cfg(debug_assertions)]`
    // fence, so in a release build both helpers are dead. They stay compiled
    // in both profiles (a future release-profile caller must not find them
    // missing) and the attribute states why the lint is expected there.
    #[cfg_attr(
        not(debug_assertions),
        expect(
            dead_code,
            reason = "only the debug-only owner-platform fence calls this"
        )
    )]
    pub(super) fn installed_ui_runtime_phase(&self) -> Option<flui_scheduler::SchedulerPhase> {
        if let Some(scheduler) = self.dispatched_scheduler.as_ref() {
            return Some(scheduler.phase());
        }
        let mut first_observed = None;
        for (_, slot) in self.ui_runtimes.iter() {
            let Some(ui_runtime) = slot.ui_runtime.as_ref() else {
                continue;
            };
            let phase = ui_runtime.scheduler().phase();
            if Self::is_frame_transaction_phase(phase) {
                return Some(phase);
            }
            first_observed.get_or_insert(phase);
        }
        first_observed
    }

    /// The earliest wall-clock instant this loop's platform event loop
    /// should wake at instead of blocking forever (issue #556's wall-clock
    /// wake) — the min over every hosted UI runtime's own
    /// [`super::ui_runtime::UiRuntime::next_wake`] (which itself is the min over
    /// that UI runtime's own presentations' armed gesture-arena deadlines).
    /// `None` when nothing anywhere needs a wall-clock wake — the loop
    /// falls back to blocking indefinitely, exactly as before this
    /// mechanism existed.
    ///
    /// **Design-for-N (N UI runtimes on one loop thread):** iterates every
    /// resident slot, same discipline as [`Self::installed_ui_runtime_phase`] —
    /// a UI runtime currently checked out for dispatch (`slot.ui_runtime` is `None`)
    /// contributes nothing to this call, which is correct: this is only
    /// ever consulted from `about_to_wait`, after every dispatch for this
    /// iteration has already returned and every UI runtime slot is back in
    /// place.
    ///
    /// Only UI runtime-owned sources participate. The former standalone gesture
    /// timer service was retired because it duplicated gesture-arena
    /// deadlines without a production drain path. Keeping this aggregate
    /// owner-local means every deadline returned here is advanced by the
    /// same UI runtime frame path that the wake re-enters.
    #[must_use]
    pub(super) fn next_wake(&self) -> Option<web_time::Instant> {
        self.ui_runtimes
            .iter()
            .filter_map(|(_, slot)| slot.ui_runtime.as_ref())
            .filter_map(super::ui_runtime::UiRuntime::next_wake)
            .min()
    }

    /// The un-deferred application of an `Install` mutation: registers
    /// a prepared native identity in the `WindowRegistry` first, strictly (never replacing an
    /// existing mapping — an id collision is refused, not silently
    /// re-routed onto a sibling UI runtime's window), and only inserts `slot`
    /// into `ui_runtimes` once that registration succeeds. Preparation already
    /// read the window identity outside app borrows; publication calls no window
    /// method. `WindowId` stays inside the registry module (ADR-0037 §2).
    ///
    /// On `Err`, hands `slot` BACK rather than dropping it here: this method
    /// is always called while some caller's `APP_RUNTIME` `RefCell` borrow is
    /// still live, and `slot` owns a whole `UiRuntime` whose destructors may
    /// re-enter platform/framework code — the same reason
    /// `install_platform_ui_runtime`/`teardown_platform_ui_runtime` never drop a
    /// UI runtime-owning value while their own TLS borrow is held. Every caller of
    /// this method routes the returned slot through its own `removed`-style
    /// return value instead, so the actual drop happens only once that
    /// borrow has released.
    fn apply_install(
        &mut self,
        id: UiRuntimeId,
        slot: RuntimeSlot,
        registration: PreparedWindowRegistration,
    ) -> Result<(), (RegistryError, Box<RuntimeSlot>)> {
        if let Err(error) = self.registry.try_register(registration, slot.address) {
            return Err((error, Box::new(slot)));
        }
        self.ui_runtimes.insert(id, slot);
        Ok(())
    }

    /// The un-deferred application of an `Uninstall` mutation.
    ///
    /// Registry removal runs first, matching `window_registry.rs`'s
    /// module-doc invariant and `teardown_platform_ui_runtime`'s own ordering:
    /// stop new routing before the returned `RuntimeSlot`'s queued
    /// old-generation events are ever dropped (whenever the caller
    /// eventually drops it — see this module's TLS-borrow-reentrancy
    /// discipline for why that drop happens outside this function, not
    /// inside it).
    fn apply_uninstall(&mut self, id: UiRuntimeId) -> Option<RuntimeSlot> {
        self.frame_drivers.retire_runtime(id);
        self.registry.remove_ui_runtime(id);
        self.native_retirement
            .close_handlers(self.close_requests.take_ui_runtime(id));
        self.ui_runtimes.remove(&id)
    }

    /// A clone of this loop's close-request router (issue #558), for the
    /// bootstrap that registers a window's handler and for the
    /// `on_should_close` closure that consults it.
    ///
    /// Handing out an `Arc` rather than a borrow is the point: the
    /// consulting closure must be able to answer a platform close request
    /// without taking a borrow on this thread-local, since a close request
    /// can arrive while a UI runtime is checked out for dispatch.
    pub(super) fn close_requests(&self) -> Arc<super::close_request::CloseRequestRouter> {
        Arc::clone(&self.close_requests)
    }

    /// Requests installing a newly-constructed UI runtime, registering `window`'s
    /// id and inserting `slot` into the registry TOGETHER, atomically from
    /// every caller's perspective: either both happen now, or both are
    /// queued as one [`RuntimeMapMutation::Install`] entry and both happen
    /// together once the queue drains. This closes the gap a two-step
    /// "register the window now, defer the UI runtime insert" sequence would
    /// otherwise leave open — a platform event addressed to the new
    /// window's id arriving in that gap would find a `WindowRegistry` entry
    /// but no matching `ui_runtimes` entry, and `dispatch_platform_ui_runtime` would
    /// misreport it as `StaleRuntime` ("a newer UI runtime replaced the one it was
    /// dispatched for") instead of "this UI runtime's install has not landed yet".
    ///
    /// Applied immediately unless a dispatch (`dispatched_ui_runtime_id` is
    /// `Some`) or an all-UI runtimes iteration (`iterating_all_ui_runtimes`) is
    /// currently in flight on this thread, in which case it queues instead —
    /// the mechanism behind two contracts: an install requested from inside a
    /// dispatched frame callback never nests into a second, concurrent
    /// dispatch (it lands once the outer dispatch's restore completes,
    /// `dispatch_platform_ui_runtime`'s tail), and a mutation requested
    /// mid-hot-restart-visit never changes the set of UI runtimes
    /// `for_each_installed_ui_runtime` is still walking (its own tail).
    ///
    /// `Err` only when applied immediately AND the window's id already maps
    /// to a live entry — deferred installs cannot fail synchronously (the
    /// caller has already returned by the time they apply); see
    /// [`Self::drain_pending_ui_runtime_mutations`] for how that case is handled
    /// instead (traced and dropped, never silently re-routed). On that
    /// immediate `Err`, hands `slot` back inside the error (see
    /// [`Self::apply_install`]'s own doc for why) — the caller (always
    /// itself inside a live `APP_RUNTIME` borrow) must drop it only after
    /// that borrow releases.
    #[cfg_attr(
        not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
        expect(
            dead_code,
            reason = "runner.rs::install_ui_runtime_alongside (its one production caller) is \
                      desktop-only -- android/wasm32 have no caller outside this crate's own tests"
        )
    )]
    pub(super) fn request_ui_runtime_install(
        &mut self,
        id: UiRuntimeId,
        slot: RuntimeSlot,
        registration: PreparedWindowRegistration,
    ) -> Result<(), (RegistryError, Box<RuntimeSlot>)> {
        if self.dispatched_ui_runtime_id.is_some() || self.iterating_all_ui_runtimes {
            self.pending_ui_runtime_mutations
                .push(RuntimeMapMutation::Install(
                    id,
                    Box::new(slot),
                    registration,
                ));
            return Ok(());
        }
        self.apply_install(id, slot, registration)
    }

    /// Requests uninstalling one UI runtime — a single window closing while
    /// siblings stay open. Same defer-to-idle discipline as
    /// [`Self::request_ui_runtime_install`]; see that method's doc for why.
    ///
    /// Returns any [`RuntimeSlot`] an IMMEDIATE uninstall removed, so the
    /// caller can drop it (and the `UiRuntime` it owns) only after releasing
    /// whatever `APP_RUNTIME` borrow is live — the same discipline
    /// `install_platform_ui_runtime`/`teardown_platform_ui_runtime` already follow for
    /// every other UI runtime-owning drop in this module.
    ///
    /// Production caller: `dispatch_platform_ui_runtime`'s `RuntimeTask::
    /// ClosePresentation` handling (`runner.rs`), when the presentation
    /// being closed is the UI runtime's only one — closing a UI runtime's sole
    /// presentation IS closing the UI runtime, so that dispatch handling routes
    /// here instead of ever leaving a live UI runtime with an empty
    /// `PresentationForest`.
    pub(super) fn request_ui_runtime_uninstall(&mut self, id: UiRuntimeId) -> Option<RuntimeSlot> {
        if self.dispatched_ui_runtime_id.is_some() || self.iterating_all_ui_runtimes {
            self.pending_ui_runtime_mutations
                .push(RuntimeMapMutation::Uninstall(id));
            return None;
        }
        self.apply_uninstall(id)
    }

    /// Applies every UI runtime-map mutation deferred while a dispatch or an
    /// all-UI runtimes iteration was in flight, in request order. Called once the
    /// deferring condition clears: the tail of `dispatch_platform_ui_runtime` and
    /// the tail of `for_each_installed_ui_runtime` (both `runner.rs`), and
    /// [`Self::should_exit`] (the drain-before-decide rule).
    ///
    /// **Early-returns (drains nothing) while `dispatched_ui_runtime_id.is_some()
    /// || iterating_all_ui_runtimes` is still true** — a NESTED call reached
    /// through one of those three call sites (e.g. a dispatch run from
    /// inside a `for_each_installed_ui_runtime` visit, which that function's own
    /// doc says is safe to attempt) must not apply a mutation an OUTER,
    /// still-in-flight operation deferred: draining it early would remove a
    /// UI runtime's slot while that UI runtime might still be the one checked out for
    /// the outer visit, and the outer visit's own restore step
    /// (`ui_runtime_slot.ui_runtime = Some(ui_runtime)` finding no slot to write into)
    /// would then silently drop a live, un-torn-down `UiRuntime` instead of
    /// restoring it. All three legitimate call sites clear their OWN flag
    /// before calling this, so the guard only ever blocks a genuinely nested
    /// caller, never the outer operation whose completion is supposed to
    /// trigger the drain.
    ///
    /// Returns every removed [`RuntimeSlot`] — both from a deferred
    /// `Uninstall`, AND from a deferred `Install` that collided on its
    /// window id — for the caller to drop outside the live `APP_RUNTIME`
    /// borrow — see [`Self::apply_install`]'s own doc for why that
    /// discipline matters here: this method runs inside every one of its
    /// three callers' live `RefCell` borrow, so it must never drop a
    /// UI runtime-owning value itself.
    pub(super) fn drain_pending_ui_runtime_mutations(&mut self) -> Vec<RuntimeSlot> {
        if self.dispatched_ui_runtime_id.is_some() || self.iterating_all_ui_runtimes {
            return Vec::new();
        }
        let pending = std::mem::take(&mut self.pending_ui_runtime_mutations);
        let mut removed = Vec::new();
        for mutation in pending {
            match mutation {
                RuntimeMapMutation::Install(id, slot, registration) => {
                    if let Err((error, slot)) = self.apply_install(id, *slot, registration) {
                        // The collided slot is routed through `removed`, the
                        // SAME bucket a rejected/removed `Uninstall` slot
                        // uses below -- never dropped here, inside this
                        // method's live borrow.
                        tracing::error!(
                            ?id,
                            ?error,
                            "dropping a deferred ui_runtime install: its window id collided with an \
                             already-registered mapping"
                        );
                        removed.push(*slot);
                    }
                }
                RuntimeMapMutation::Uninstall(id) => {
                    if let Some(slot) = self.apply_uninstall(id) {
                        removed.push(slot);
                    }
                }
            }
        }
        removed
    }

    /// Teardown-only introspection: whether any UI runtime-map mutation is still
    /// waiting to be applied. Should always be `false` by the time
    /// `teardown_platform_ui_runtime` (full loop-exit teardown) runs: both
    /// `dispatch_platform_ui_runtime` and `for_each_installed_ui_runtime` drain
    /// unconditionally in their own tails (panic or not), so nothing should
    /// ever still be queued once every in-flight dispatch/iteration has
    /// completed. `teardown_platform_ui_runtime` asserts this rather than
    /// silently dropping a still-pending mutation (and the `UiRuntime` an
    /// `Install` mutation might still be holding) unnoticed.
    ///
    /// `cfg`-gated to match that one caller exactly: `teardown_platform_ui_runtime`
    /// is `#[cfg(not(target_arch = "wasm32"))]`
    /// (the web host never tears down at all — see that function's own
    /// module doc), so on wasm32 this method has no caller at all and must
    /// not compile there either, or it is dead code under `wasm-check`'s
    /// deny-warnings build.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn has_pending_ui_runtime_mutations(&self) -> bool {
        !self.pending_ui_runtime_mutations.is_empty()
    }

    /// Whether the loop should exit under `policy`, given the UI runtimes
    /// currently hosted, plus any [`RuntimeSlot`]s a deferred mutation just
    /// removed (drop these outside the `APP_RUNTIME` borrow, same as every
    /// other UI runtime-owning drop in this module).
    ///
    /// **Drain-before-decide:** calls
    /// [`Self::drain_pending_ui_runtime_mutations`] BEFORE checking `policy` — a
    /// queued install (e.g. a splash screen's dispose callback requesting the
    /// main window) is applied first, so it vetoes exit instead of racing the
    /// empty-registry check. Without that ordering, a splash-close and a
    /// main-window-open landing in the same idle tick could observe "no
    /// UI runtimes installed" and exit before the queued install ever lands.
    #[cfg_attr(
        not(any(
            test,
            all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            )
        )),
        expect(
            dead_code,
            reason = "runner.rs::install_exit_policy_hook (its one production caller) is \
                      desktop-only -- android/wasm32 have no caller outside this crate's own tests"
        )
    )]
    pub(super) fn should_exit(&mut self, policy: ExitPolicy) -> (bool, Vec<RuntimeSlot>) {
        let removed = self.drain_pending_ui_runtime_mutations();
        let exit = match policy {
            ExitPolicy::ExplicitQuit => false,
            // A running service that declared `ServiceLifetime::KeepsAppAlive`
            // (issue #558) vetoes exit the same way a queued install does:
            // messenger-like applications survive their last window closing
            // and end when the service completes or the embedder calls
            // `Platform::quit` explicitly. Not compiled on wasm32 — the
            // lifecycle layer is native-only there and the web loop never
            // exits anyway.
            #[cfg(not(target_arch = "wasm32"))]
            ExitPolicy::OnLastWindowClosed => {
                self.ui_runtimes.is_empty()
                    && self
                        .pending_window_reservations
                        .load(std::sync::atomic::Ordering::Acquire)
                        == 0
                    && !self.service_registry.keeps_app_alive()
            }
            #[cfg(target_arch = "wasm32")]
            ExitPolicy::OnLastWindowClosed => self.ui_runtimes.is_empty(),
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

    /// Owner-thread poke used only to continue bounded owner work. Unlike a
    /// frame wake it does not mark the UI runtime dirty; operations in the batch
    /// request a frame themselves when their effects require one.
    #[cfg(target_os = "android")]
    pub(super) fn owner_turn_window_poke(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let redraw_window = Arc::clone(&self.redraw_window);
        Arc::new(move || {
            let window = redraw_window.lock().as_ref().cloned();
            let Some(window) = window else {
                return false;
            };
            if window.execution_state() != WindowExecutionState::Running {
                return false;
            }
            window.request_redraw();
            true
        })
    }

    /// A `Send + Sync`, `'static` capability that sets `needs_redraw` and
    /// pokes the installed window — safe to hand to an `UpdateScheduler` lifecycle
    /// hook, an `on_frame_scheduled` hook, or a spawned future's `Waker`,
    /// none of which may resolve this thread-local `AppRuntime` at fire time
    /// (see [`FrameWakeHandle`]'s doc).
    pub(super) fn frame_wake_callback(&self) -> Arc<dyn Fn() + Send + Sync> {
        let wake = self.wake_handle().into_callback();
        #[cfg(target_os = "ios")]
        if let Some(owner) = &self.owner_platform {
            let proxy = owner.proxy();
            return Arc::new(move || {
                wake();
                let _ = proxy.wake();
            });
        }
        wake
    }

    /// Wake the platform event loop so the next frame is rendered: sets
    /// `needs_redraw` and, if a window is installed, calls
    /// `PlatformWindow::request_redraw()` so a quiescent event loop wakes up.
    ///
    /// Every production bootstrap wires and calls this indirectly through
    /// [`Self::frame_wake_callback`]'s `Send + Sync` closure instead of this
    /// direct form (the closure is what a `UiRuntime`/cross-thread hook needs
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
        reason = "production redraw requests are ui_runtime-scoped \
                  (UiRuntime::request_redraw, sharing this same needs_redraw \
                  atomic via needs_redraw_handle); no loop-scoped caller \
                  needs the direct form yet, and no test exercises the \
                  flag-only form in isolation from wake_frame either"
    )]
    pub(super) fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// A clone of the `needs_redraw` flag, for a `UiRuntime`'s own
    /// flag-only redraw requests (its `attach_root_widget`/
    /// `handle_input_entered` call sites) — the SAME atomic this runtime
    /// reads, so either side observes the other's writes.
    pub(super) fn needs_redraw_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.needs_redraw)
    }

    /// Install the window [`Self::wake_frame`] pokes. Called once the
    /// UI runtime's window is open, before anything that could synchronously
    /// observe it (the initial redraw request, `Lifecycle::Started`) runs —
    /// otherwise the first such observer would silently see no window.
    pub(super) fn set_redraw_window(&self, window: Arc<dyn PlatformWindow>) {
        let _prev = self.redraw_window.lock().replace(window);
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
    /// UI runtime's window is not kept artificially alive by this slot.
    ///
    /// Returns the removed window instead of dropping it under the lock:
    /// the caller drops it only after every TLS borrow has been released
    /// (destructors may re-enter platform/framework code), and — for the
    /// post-loop `teardown_platform_ui_runtime` caller — with the knowledge that
    /// the platform event loop is already gone, which is why the primary
    /// release path is [`Self::release_redraw_window_for`] at window close,
    /// while the loop is still alive.
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "only teardown_platform_ui_runtime calls this, and that function \
                      is desktop/android-only (the web backend's host stays \
                      resident for the page's lifetime)"
        )
    )]
    #[must_use = "drop the returned window only after releasing TLS borrows"]
    pub(super) fn clear_redraw_window(&self) -> Option<Arc<dyn PlatformWindow>> {
        self.redraw_window.lock().take()
    }

    /// Remove the installed redraw-poke window if (and only if) it is the
    /// window identified by `id` — the window-close release path.
    ///
    /// Teardown-order invariant (issue #713): every `Arc` of a platform
    /// window must unwind while the platform event loop is still alive —
    /// this slot was the one reference that survived `Platform::run` and
    /// forced the window's native teardown to run after the loop (and, on
    /// Wayland, after the connection state it marshals on) was gone. Called
    /// from the window's own `on_close`, so a `WindowPolicy::Shared` sibling closing
    /// some *other* window leaves the slot untouched. Returns the removed
    /// window for the caller to drop outside any TLS borrow.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    #[must_use = "drop the returned window only after releasing TLS borrows"]
    pub(super) fn release_redraw_window_for(
        &self,
        id: flui_platform::traits::WindowId,
    ) -> Option<Arc<dyn PlatformWindow>> {
        let mut slot = self.redraw_window.lock();
        if slot.as_ref().is_some_and(|window| window.id() == id) {
            slot.take()
        } else {
            None
        }
    }

    // ========================================================================
    // Clipboard, moved from the retired `AppBinding`
    // ========================================================================

    /// Install the platform's clipboard capability. See `AppBinding`'s
    /// former doc (now this field's) for why this is a plain slot rather
    /// than a new `Platform` surface.
    pub(super) fn set_platform_clipboard(&self, clipboard: Arc<dyn Clipboard>) {
        let _prev = self.platform_clipboard.lock().replace(clipboard);
    }

    /// The explicit, deterministic teardown clear — the first of the two
    /// non-last-resort clear paths (the second is the bootstrap `set` itself
    /// replacing a prior installation; see [`Drop`]'s impl for the third,
    /// last-resort path).
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "only teardown_platform_ui_runtime calls this, and that function \
                      is desktop/android-only (the web backend's host stays \
                      resident for the page's lifetime)"
        )
    )]
    pub(super) fn clear_platform_clipboard(&self) {
        let _prev = self.platform_clipboard.lock().take();
    }

    /// Resolve the host's byte storage from the run's `config`, once, when
    /// the host starts: every UI runtime the runners and secondary windows build
    /// afterwards takes this one (`runner::host::build_ui_runtime`).
    pub(super) fn install_host_storage(&mut self, config: &super::AppConfig) {
        self.host_storage = super::storage_host::host_storage(config);
    }

    /// The host's byte storage, if the run configured one.
    pub(super) fn host_storage(&self) -> Option<Arc<dyn flui_platform_api::Storage>> {
        self.host_storage.clone()
    }

    /// Access the installed platform clipboard, if any. Every runner reads it
    /// through `runner::host::runtime_clipboard` to hand each UI runtime it builds
    /// the platform clipboard.
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
}

/// How [`AppRuntime::resolved_services`] starts the host font feed, given
/// the feed and the wake to call once it has landed: on a thread of its own
/// ([`spawn_host_feed`]). This crate's unit tests park it instead
/// (`park_host_feed`), so no feed lands in the middle of a test that
/// counts generations or redraws; `spawn_host_feed` has its own test.
#[cfg(not(test))]
const LAUNCH_HOST_FEED: fn(HostFontFeed, Arc<dyn Fn() + Send + Sync>) = spawn_host_feed;
#[cfg(test)]
const LAUNCH_HOST_FEED: fn(HostFontFeed, Arc<dyn Fn() + Send + Sync>) = park_host_feed;

/// A feed [`park_host_feed`] held, with the wake the runtime launched it
/// with: a test runs the feed, then calls the wake as the launcher would.
#[cfg(test)]
pub(super) struct ParkedHostFeed {
    pub(super) feed: HostFontFeed,
    pub(super) wake: Arc<dyn Fn() + Send + Sync>,
}

#[cfg(test)]
thread_local! {
    /// The feeds [`park_host_feed`] held on this thread, in launch order.
    static PARKED_HOST_FEEDS: RefCell<Vec<ParkedHostFeed>> = const { RefCell::new(Vec::new()) };
}

/// A launcher that runs nothing: it keeps the feed and its wake on this
/// thread for [`take_parked_host_feeds`], so a test decides when, and
/// whether, the host's faces land.
#[cfg(test)]
pub(super) fn park_host_feed(feed: HostFontFeed, wake: Arc<dyn Fn() + Send + Sync>) {
    PARKED_HOST_FEEDS.with(|parked| parked.borrow_mut().push(ParkedHostFeed { feed, wake }));
}

/// Every feed [`park_host_feed`] has held on this thread since the last
/// call, in launch order.
#[cfg(test)]
pub(super) fn take_parked_host_feeds() -> Vec<ParkedHostFeed> {
    PARKED_HOST_FEEDS.with(|parked| std::mem::take(&mut *parked.borrow_mut()))
}

/// Runs the host font feed on a thread of its own, named `flui-host-fonts`,
/// then wakes the owner, whose next turn tells every UI runtime.
///
/// The wake is called even if the feed panics, since the generation still
/// rises then. If no thread can be started the feed runs here, before the
/// first frame; on wasm32, which has no threads (and where fontdb finds no
/// host fonts), it always does.
fn spawn_host_feed(feed: HostFontFeed, wake: Arc<dyn Fn() + Send + Sync>) {
    fn feed_then_wake(feed: HostFontFeed, wake: &(dyn Fn() + Send + Sync)) {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        if catch_unwind(AssertUnwindSafe(|| feed.run())).is_err() {
            tracing::error!("the host font feed panicked; the faces it added are kept");
        }
        if catch_unwind(AssertUnwindSafe(wake)).is_err() {
            tracing::error!("the wake after the host font feed panicked");
        }
    }

    #[cfg(target_arch = "wasm32")]
    feed_then_wake(feed, &*wake);
    #[cfg(not(target_arch = "wasm32"))]
    {
        // The feed goes to the thread only once it has started, so a failed
        // start leaves it here to run inline.
        let (send, receive) = std::sync::mpsc::channel::<HostFontFeed>();
        let thread_wake = Arc::clone(&wake);
        let spawned = std::thread::Builder::new()
            .name("flui-host-fonts".to_owned())
            .spawn(move || {
                if let Ok(feed) = receive.recv() {
                    feed_then_wake(feed, &*thread_wake);
                }
            });
        match spawned {
            Ok(_) => {
                if let Err(std::sync::mpsc::SendError(feed)) = send.send(feed) {
                    tracing::warn!("the host font thread ended before its feed; feeding here");
                    feed_then_wake(feed, &*wake);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "no thread for the host font feed; feeding here");
                feed_then_wake(feed, &*wake);
            }
        }
    }
}

impl Drop for AppRuntime {
    /// The third, last-resort clipboard clear: the deterministic path is the explicit
    /// `teardown_platform_ui_runtime` clear; this is only a backstop for
    /// whatever construction/panic ordering skips it. Idempotent — clearing
    /// an already-empty slot is a no-op — and must never assert platform
    /// presence: a thread-local's destructor is not guaranteed to run in any
    /// particular order relative to window/surface teardown, so this may
    /// run before, after, or never relative to those.
    fn drop(&mut self) {
        let _prev = self.platform_clipboard.lock().take();
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod font_collection_tests {
    use flui_painting::testing::{PROBE_MONO_100, feed_with_host, host_fed, host_fonts_from};

    use super::*;

    /// Every UI runtime builds its `TextContext` over the app's one collection
    /// (ADR-0092 §2), and the host's faces are fed into it once, off the
    /// owner thread (ADR-0092 §7): repeated service and collection requests
    /// hand out clones of the same collection, the runtime launches one
    /// feed for it, and that collection is host-fed once the feed runs. The
    /// wake the feed is launched with is the runtime's own: calling it asks
    /// the loop for the turn that announces the landing. Fails if the
    /// services feed the host on the owner thread (the collection is
    /// host-fed before the launch), launch a feed per request, build a new
    /// collection per request, or launch the feed with a wake that does not
    /// reach the loop.
    fn the_runtime_launches_one_host_feed_for_every_ui_runtime() {
        let _ = take_parked_host_feeds();
        let mut runtime = AppRuntime::new();
        let first = runtime.font_collection();
        let _ = runtime.ensure_services();
        let _ = runtime.ensure_services();
        assert!(
            FontCollection::ptr_eq(&first, &runtime.font_collection()),
            "every ui_runtime must get the app's one font collection, not a fresh one per call"
        );
        assert!(
            FontCollection::ptr_eq(&first, &runtime.ensure_services().fonts),
            "the collection handed to ui_runtimes is the one the services own"
        );
        assert!(
            !host_fed(&first),
            "the first frame does not wait for the host's faces"
        );
        let mut feeds = take_parked_host_feeds();
        assert_eq!(feeds.len(), 1, "one feed per app");
        let ParkedHostFeed { feed, wake } = feeds.remove(0);
        runtime.needs_redraw.store(false, Ordering::Relaxed);

        feed_with_host(feed, host_fonts_from(&[PROBE_MONO_100])).run();
        wake();

        assert!(
            runtime.needs_redraw.load(Ordering::Relaxed),
            "the feed's wake asks the runtime's loop for a turn"
        );
        assert!(host_fed(&first), "the feed fed the app's collection");
        assert!(runtime.take_font_change(), "the landing is announced");
        assert!(!runtime.take_font_change(), "and announced once");
    }

    /// The production launcher runs the feed on another thread and then
    /// calls the wake once, from that thread. Fails if the launcher feeds
    /// on the calling thread, which is the owner's.
    fn the_host_feed_runs_off_the_owner_thread_and_wakes_once() {
        let (fonts, feed) = FontCollection::with_host_feed();
        let feed = feed_with_host(feed, host_fonts_from(&[PROBE_MONO_100]));
        let (send, woken) = std::sync::mpsc::channel();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = send.send(std::thread::current().id());
        });

        spawn_host_feed(feed, wake);

        let waker = woken
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("the feed wakes the owner once it lands");
        assert_ne!(
            waker,
            std::thread::current().id(),
            "the feed ran off the owner thread"
        );
        assert!(host_fed(&fonts), "the wake comes after the feed");
        assert!(
            woken.recv().is_err(),
            "the thread ends without a second wake"
        );
    }

    /// Two runtimes, as on two owner threads, each hold a collection of
    /// their own. Fails if the services share one collection between
    /// runtimes.
    fn two_runtimes_hold_different_collections() {
        let one = AppRuntime::new();
        let two = AppRuntime::new();

        assert!(!FontCollection::ptr_eq(
            &one.font_collection(),
            &two.font_collection()
        ));
        let _ = take_parked_host_feeds();
    }

    #[test]
    fn font_collection_contract() {
        crate::table_test::run_table(
            "font_collection_contract",
            &[
                (
                    "the_runtime_launches_one_host_feed_for_every_ui_runtime",
                    the_runtime_launches_one_host_feed_for_every_ui_runtime as fn(),
                ),
                (
                    "the_host_feed_runs_off_the_owner_thread_and_wakes_once",
                    the_host_feed_runs_off_the_owner_thread_and_wakes_once as fn(),
                ),
                (
                    "two_runtimes_hold_different_collections",
                    two_runtimes_hold_different_collections as fn(),
                ),
            ],
        );
    }
}

// `crate::app::lifecycle` (Task/Worker/Service) is `cfg(not(wasm32))`, so
// every test in here is about a seam that does not exist on wasm32.
#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod service_lifecycle_wiring_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::app::lifecycle::{ServiceDefinition, ServiceLifetime};
    use flui_runtime::execution::DeterministicExecutors;

    /// The editor/messenger acceptance split at the runtime seam: with no
    /// UI runtimes hosted, `should_exit(OnLastWindowClosed)` says exit — unless
    /// a running `KeepsAppAlive` service vetoes it; once that service
    /// completes, the veto lifts. `StopsWithLastWindow` services never
    /// veto. Fails without `should_exit`'s registry consult.
    #[test]
    fn keep_alive_service_vetoes_exit_until_it_completes() {
        let deterministic = DeterministicExecutors::new();
        let mut runtime = AppRuntime::new();
        runtime.install_host_executors(deterministic.host_executors());

        // Editor-like: a StopsWithLastWindow service does not hold exit.
        runtime
            .start_service(&ServiceDefinition::new(
                "editor-autosave",
                ServiceLifetime::StopsWithLastWindow,
                |context| {
                    let signal = context.cancellation().clone();
                    Box::pin(async move { signal.cancelled().await })
                },
            ))
            .expect("service must start");
        let (exit, removed) = runtime.should_exit(ExitPolicy::OnLastWindowClosed);
        drop(removed);
        assert!(
            exit,
            "no ui_runtimes + only editor-like services: the loop must exit"
        );

        // Messenger-like: a running KeepsAppAlive service vetoes exit. The
        // service parks (storing its waker) until the test releases it —
        // the same park/wake discipline the deterministic executor's own
        // tests use.
        let release = Arc::new(AtomicBool::new(false));
        let waker_slot: Arc<parking_lot::Mutex<Option<std::task::Waker>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let release_in_service = Arc::clone(&release);
        let waker_in_service = Arc::clone(&waker_slot);
        runtime
            .start_service(&ServiceDefinition::new(
                "messenger-sync",
                ServiceLifetime::KeepsAppAlive,
                move |_context| {
                    let release = Arc::clone(&release_in_service);
                    let waker_slot = Arc::clone(&waker_in_service);
                    Box::pin(async move {
                        std::future::poll_fn(move |context| {
                            if release.load(Ordering::Acquire) {
                                std::task::Poll::Ready(())
                            } else {
                                let _prev = waker_slot.lock().replace(context.waker().clone());
                                std::task::Poll::Pending
                            }
                        })
                        .await;
                    })
                },
            ))
            .expect("service must start");
        deterministic.run_until_idle();
        let (exit, removed) = runtime.should_exit(ExitPolicy::OnLastWindowClosed);
        drop(removed);
        assert!(
            !exit,
            "a running keep-alive service must veto exit after the last window closes"
        );

        // The service completing lifts the veto with no other change.
        release.store(true, Ordering::Release);
        waker_slot
            .lock()
            .take()
            .expect("the parked service stored its waker")
            .wake();
        deterministic.run_until_idle();
        let (exit, removed) = runtime.should_exit(ExitPolicy::OnLastWindowClosed);
        drop(removed);
        assert!(
            exit,
            "a completed keep-alive service must not hold the loop"
        );
    }
}
